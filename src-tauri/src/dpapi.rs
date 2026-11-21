use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::ffi::c_void;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::*;
use windows::core::PCWSTR;

pub fn protect(plain: &str) -> Result<String> {
  let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
  let mut out = CRYPT_INTEGER_BLOB::default();
  // SAFETY: input points at live bytes for the call; output is copied then freed with LocalFree.
  unsafe {
    CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out.pbData as *mut c_void)));
    Ok(STANDARD.encode(bytes))
  }
}

pub fn unprotect(b64: &str) -> Result<String> {
  let mut cipher = STANDARD.decode(b64)?;
  let input = CRYPT_INTEGER_BLOB { cbData: cipher.len() as u32, pbData: cipher.as_mut_ptr() };
  let mut out = CRYPT_INTEGER_BLOB::default();
  // SAFETY: as in protect.
  unsafe {
    CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out.pbData as *mut c_void)));
    Ok(String::from_utf8(bytes)?)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn dpapi_round_trip() {
    assert_eq!(unprotect(&protect("_|WARNING:x|_abc").unwrap()).unwrap(), "_|WARNING:x|_abc");
  }

  #[test]
  fn dpapi_garbage_errors() {
    assert!(unprotect("bm90IGRwYXBp").is_err());
  }
}
