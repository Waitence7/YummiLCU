use base64::{engine::general_purpose::STANDARD as B64, Engine};

pub(super) fn protect(value: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

        let input = CRYPT_INTEGER_BLOB {
            cbData: value.len() as u32,
            pbData: value.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, None, None, None, None, 0, &mut output).ok()?;
            let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
            Some(B64.encode(bytes))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        None
    }
}

pub(super) fn unprotect(value: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

        let bytes = B64.decode(value).ok()?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&input, None, None, None, None, 0, &mut output).ok()?;
            Some(
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    output.pbData,
                    output.cbData as usize,
                ))
                .to_string(),
            )
        }
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        None
    }
}
