use base64::{engine::general_purpose::STANDARD as B64, Engine};

pub(super) fn protect(value: &[u8]) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{
                CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
            },
        };

        if value.len() > u32::MAX as usize {
            return None;
        }

        let input = CRYPT_INTEGER_BLOB {
            cbData: value.len() as u32,
            pbData: value.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .ok()?;
            if output.pbData.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
            let encoded = B64.encode(bytes);
            let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            Some(encoded)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        None
    }
}

pub(super) fn unprotect(value: &str) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{
                CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
            },
        };

        let bytes = B64.decode(value).ok()?;
        if bytes.len() > u32::MAX as usize {
            return None;
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .ok()?;
            if output.pbData.is_null() {
                return None;
            }
            let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
            let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            Some(plain)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        None
    }
}
