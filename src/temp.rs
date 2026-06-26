//! CPU temperature from the SMC (System Management Controller) over IOKit. Reading SMC keys needs
//! no elevated privileges — this is how iStat Menus and `osx-cpu-temp` work. It is Intel-Mac
//! territory: the temperature keys and the `sp78` encoding here are the classic Intel SMC; on
//! Apple Silicon the keys differ and every read fails, so this returns `None` and the segment
//! simply vanishes (the same graceful degradation as the throttle).

/// CPU temperature in °C, best-effort. Tries the usual CPU sensor keys in order.
#[cfg(target_os = "macos")]
pub fn cpu_celsius() -> Option<f64> {
    imp::read_first(&["TC0P", "TC0D", "TC0E", "TC0F", "TCAD"])
}

#[cfg(not(target_os = "macos"))]
pub fn cpu_celsius() -> Option<f64> {
    None
}

#[cfg(target_os = "macos")]
mod imp {
    use std::os::raw::{c_char, c_int, c_void};

    type IoConnect = u32;
    type IoService = u32;
    type KernReturn = c_int;
    type MachPort = u32;

    const KERNEL_INDEX_SMC: u32 = 2;
    const SMC_CMD_READ_BYTES: u8 = 5;
    const SMC_CMD_READ_KEYINFO: u8 = 9;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(master_port: MachPort, matching: *mut c_void) -> IoService;
        fn IOServiceOpen(
            service: IoService,
            owning_task: MachPort,
            ty: u32,
            conn: *mut IoConnect,
        ) -> KernReturn;
        fn IOServiceClose(conn: IoConnect) -> KernReturn;
        fn IOObjectRelease(obj: IoService) -> KernReturn;
        fn IOConnectCallStructMethod(
            conn: IoConnect,
            selector: u32,
            input: *const c_void,
            input_size: usize,
            output: *mut c_void,
            output_size: *mut usize,
        ) -> KernReturn;
    }
    extern "C" {
        // The task-self send right (a global in libSystem; `mach_task_self()` is a macro over it).
        static mach_task_self_: MachPort;
    }

    // Layout mirrors the C `SMCKeyData_t` exactly (sizeof == 80); see osx-cpu-temp / Apple's smc.c.
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Vers {
        major: u8,
        minor: u8,
        build: u8,
        reserved: u8,
        release: u16,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct PLimitData {
        version: u16,
        length: u16,
        cpu_plimit: u32,
        gpu_plimit: u32,
        mem_plimit: u32,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct KeyInfo {
        data_size: u32,
        data_type: u32,
        data_attributes: u8,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct KeyData {
        key: u32,
        vers: Vers,
        p_limit: PLimitData,
        key_info: KeyInfo,
        result: u8,
        status: u8,
        data8: u8,
        data32: u32,
        bytes: [u8; 32],
    }

    fn fourcc(s: &str) -> u32 {
        let b = s.as_bytes();
        ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32)
    }

    struct Smc(IoConnect);

    impl Smc {
        fn open() -> Option<Smc> {
            // SAFETY: the standard IOKit service lookup/open sequence; every handle is checked and
            // the matching dictionary is consumed by IOServiceGetMatchingService (no leak).
            unsafe {
                let matching = IOServiceMatching(c"AppleSMC".as_ptr());
                if matching.is_null() {
                    return None;
                }
                let service = IOServiceGetMatchingService(0, matching);
                if service == 0 {
                    return None;
                }
                let mut conn: IoConnect = 0;
                let kr = IOServiceOpen(service, mach_task_self_, 0, &mut conn);
                IOObjectRelease(service);
                if kr != 0 || conn == 0 {
                    return None;
                }
                Some(Smc(conn))
            }
        }

        fn call(&self, input: &KeyData) -> Option<KeyData> {
            let mut output = KeyData::default();
            let mut out_size = std::mem::size_of::<KeyData>();
            // SAFETY: fixed-size in/out structs whose layout matches the kernel's SMCKeyData_t.
            let kr = unsafe {
                IOConnectCallStructMethod(
                    self.0,
                    KERNEL_INDEX_SMC,
                    input as *const KeyData as *const c_void,
                    std::mem::size_of::<KeyData>(),
                    &mut output as *mut KeyData as *mut c_void,
                    &mut out_size,
                )
            };
            if kr != 0 {
                None
            } else {
                Some(output)
            }
        }

        fn read_key(&self, key: &str) -> Option<f64> {
            // 1) key info: data size + type.
            let mut input = KeyData {
                key: fourcc(key),
                data8: SMC_CMD_READ_KEYINFO,
                ..Default::default()
            };
            let info = self.call(&input)?;
            let size = info.key_info.data_size as usize;
            if size == 0 || size > 32 {
                return None;
            }
            // 2) the value bytes.
            input.key_info.data_size = info.key_info.data_size;
            input.data8 = SMC_CMD_READ_BYTES;
            let out = self.call(&input)?;
            decode(info.key_info.data_type, &out.bytes[..size])
        }
    }

    impl Drop for Smc {
        fn drop(&mut self) {
            // SAFETY: closing the connection we opened.
            unsafe {
                IOServiceClose(self.0);
            }
        }
    }

    /// Decode a temperature by its SMC data type. `sp78` is signed fixed-point 8.8 (the classic
    /// Intel temp encoding, big-endian); `flt ` is a 4-byte IEEE float (some newer sensors).
    fn decode(data_type: u32, bytes: &[u8]) -> Option<f64> {
        if data_type == fourcc("sp78") && bytes.len() >= 2 {
            Some(i16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 256.0)
        } else if data_type == fourcc("flt ") && bytes.len() == 4 {
            Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64)
        } else {
            None
        }
    }

    pub fn read_first(keys: &[&str]) -> Option<f64> {
        let smc = Smc::open()?;
        for k in keys {
            if let Some(t) = smc.read_key(k) {
                // Plausibility guard — drop bogus reads (0 / disconnected sensors / wrong type).
                if t > 1.0 && t < 130.0 {
                    return Some(t);
                }
            }
        }
        None
    }
}
