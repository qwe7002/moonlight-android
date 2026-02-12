//! Crypto implementation for moonlight-common-c
//!
//! This module provides crypto functions required by moonlight-common-c,
//! implemented using the ring crate for better cross-compilation support.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM};
use ring::digest::{digest, SHA256};
use ring::hmac;
use ring::rand::SecureRandom;
use std::ptr;
use log::{error, debug};

// AES-CBC support
use aes::Aes128;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use cbc::{Decryptor, Encryptor};

type Aes128CbcEnc = Encryptor<Aes128>;
type Aes128CbcDec = Decryptor<Aes128>;

// Algorithm constants matching PlatformCrypto.h
const ALGORITHM_AES_CBC: i32 = 1;
const ALGORITHM_AES_GCM: i32 = 2;

// Cipher flags matching PlatformCrypto.h
#[allow(dead_code)]
const CIPHER_FLAG_RESET_IV: i32 = 0x01;
const CIPHER_FLAG_FINISH: i32 = 0x02;
const CIPHER_FLAG_PAD_TO_BLOCK_SIZE: i32 = 0x04;

/// AES-128-GCM encryption context
#[allow(dead_code)]
struct AesGcmContext {
    key: LessSafeKey,
}

/// HMAC-SHA256 context
struct HmacContext {
    key: hmac::Key,
}

/// Platform crypto context (matches PLT_CRYPTO_CONTEXT in PlatformCrypto.h)
/// This is a general-purpose context that can handle both AES-CBC and AES-GCM
#[repr(C)]
pub struct PltCryptoContext {
    initialized: bool,
    // We store the key for later use since ring requires creating new contexts per operation
    key_data: [u8; 32],
    key_len: usize,
}

// ============================================================================
// New Platform Crypto API (required by moonlight-common-c)
// ============================================================================

/// Create a platform crypto context
#[no_mangle]
pub extern "C" fn PltCreateCryptoContext() -> *mut PltCryptoContext {
    debug!("PltCreateCryptoContext called");
    let context = Box::new(PltCryptoContext {
        initialized: false,
        key_data: [0u8; 32],
        key_len: 0,
    });
    Box::into_raw(context)
}

/// Destroy a platform crypto context
#[no_mangle]
pub extern "C" fn PltDestroyCryptoContext(ctx: *mut PltCryptoContext) {
    if !ctx.is_null() {
        unsafe {
            drop(Box::from_raw(ctx));
        }
    }
}

/// Encrypt a message using the specified algorithm
#[no_mangle]
pub extern "C" fn PltEncryptMessage(
    ctx: *mut PltCryptoContext,
    algorithm: i32,
    flags: i32,
    key: *mut u8,
    key_length: i32,
    iv: *mut u8,
    iv_length: i32,
    tag: *mut u8,
    tag_length: i32,
    input_data: *mut u8,
    input_data_length: i32,
    output_data: *mut u8,
    output_data_length: *mut i32,
) -> bool {
   // debug!("PltEncryptMessage called: algorithm={}, key_len={}, iv_len={}, data_len={}",
           // algorithm, key_length, iv_length, input_data_length);

    if ctx.is_null() || key.is_null() || iv.is_null() || output_data_length.is_null() {
        error!("PltEncryptMessage: null pointer argument");
        return false;
    }

    if input_data_length > 0 && (input_data.is_null() || output_data.is_null()) {
        return false;
    }

    match algorithm {
        ALGORITHM_AES_GCM => {
            // AES-GCM encryption
            if key_length != 16 || iv_length != 12 || tag.is_null() || tag_length != 16 {
                return false;
            }

            let key_slice = unsafe { std::slice::from_raw_parts(key, key_length as usize) };
            let iv_slice = unsafe { std::slice::from_raw_parts(iv, iv_length as usize) };

            let unbound_key = match UnboundKey::new(&AES_128_GCM, key_slice) {
                Ok(k) => k,
                Err(_) => return false,
            };
            let less_safe_key = LessSafeKey::new(unbound_key);

            let nonce = match Nonce::try_assume_unique_for_key(iv_slice) {
                Ok(n) => n,
                Err(_) => return false,
            };

            // Prepare buffer for in-place encryption
            let mut in_out = vec![0u8; input_data_length as usize];
            if input_data_length > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(input_data, in_out.as_mut_ptr(), input_data_length as usize);
                }
            }

            match less_safe_key.seal_in_place_separate_tag(nonce, Aad::empty(), &mut in_out) {
                Ok(actual_tag) => {
                    // Copy ciphertext
                    if input_data_length > 0 {
                        unsafe {
                            ptr::copy_nonoverlapping(in_out.as_ptr(), output_data, input_data_length as usize);
                        }
                    }
                    // Copy tag
                    unsafe {
                        ptr::copy_nonoverlapping(actual_tag.as_ref().as_ptr(), tag, tag_length as usize);
                        *output_data_length = input_data_length;
                    }
                    true
                }
                Err(_) => false,
            }
        }
        ALGORITHM_AES_CBC => {
            // AES-CBC encryption
            if key_length != 16 || iv_length != 16 {
                error!("PltEncryptMessage: invalid key/iv length for AES-CBC");
                return false;
            }

            let key_slice = unsafe { std::slice::from_raw_parts(key, key_length as usize) };
            let iv_slice = unsafe { std::slice::from_raw_parts(iv, iv_length as usize) };

            // Calculate padded length if needed
            let block_size = 16usize;
            let input_len = input_data_length as usize;
            let padded_len = if (flags & CIPHER_FLAG_PAD_TO_BLOCK_SIZE) != 0 {
                ((input_len + block_size) / block_size) * block_size
            } else {
                // Input must be multiple of block size
                if input_len % block_size != 0 {
                    error!("PltEncryptMessage: input length not multiple of 16 for AES-CBC without padding");
                    return false;
                }
                input_len
            };

            // Create buffer for encryption
            let mut buffer = vec![0u8; padded_len];
            if input_len > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(input_data, buffer.as_mut_ptr(), input_len);
                }
            }

            // Create encryptor and encrypt
            let encryptor = Aes128CbcEnc::new_from_slices(key_slice, iv_slice);
            match encryptor {
                Ok(enc) => {
                    let result = if (flags & CIPHER_FLAG_PAD_TO_BLOCK_SIZE) != 0 {
                        enc.encrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buffer, input_len)
                    } else {
                        enc.encrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buffer, input_len)
                    };

                    match result {
                        Ok(encrypted) => {
                            let encrypted_len = encrypted.len();
                            unsafe {
                                ptr::copy_nonoverlapping(encrypted.as_ptr(), output_data, encrypted_len);
                                *output_data_length = encrypted_len as i32;
                            }
                            true
                        }
                        Err(_) => {
                            error!("PltEncryptMessage: AES-CBC encryption failed");
                            false
                        }
                    }
                }
                Err(_) => {
                    error!("PltEncryptMessage: failed to create AES-CBC encryptor");
                    false
                }
            }
        }
        _ => false,
    }
}

/// Decrypt a message using the specified algorithm
#[no_mangle]
pub extern "C" fn PltDecryptMessage(
    ctx: *mut PltCryptoContext,
    algorithm: i32,
    flags: i32,
    key: *mut u8,
    key_length: i32,
    iv: *mut u8,
    iv_length: i32,
    tag: *mut u8,
    tag_length: i32,
    input_data: *mut u8,
    input_data_length: i32,
    output_data: *mut u8,
    output_data_length: *mut i32,
) -> bool {
    //debug!("PltDecryptMessage called: algorithm={}, key_len={}, iv_len={}, data_len={}",
           //algorithm, key_length, iv_length, input_data_length);

    if ctx.is_null() || key.is_null() || iv.is_null() || output_data_length.is_null() {
        error!("PltDecryptMessage: null pointer argument");
        return false;
    }

    if input_data_length > 0 && (input_data.is_null() || output_data.is_null()) {
        return false;
    }

    match algorithm {
        ALGORITHM_AES_GCM => {
            // AES-GCM decryption
            if key_length != 16 || iv_length != 12 || tag.is_null() || tag_length != 16 {
                return false;
            }

            let key_slice = unsafe { std::slice::from_raw_parts(key, key_length as usize) };
            let iv_slice = unsafe { std::slice::from_raw_parts(iv, iv_length as usize) };

            let unbound_key = match UnboundKey::new(&AES_128_GCM, key_slice) {
                Ok(k) => k,
                Err(_) => return false,
            };
            let less_safe_key = LessSafeKey::new(unbound_key);

            let nonce = match Nonce::try_assume_unique_for_key(iv_slice) {
                Ok(n) => n,
                Err(_) => return false,
            };

            // Combine ciphertext and tag for decryption
            let total_len = input_data_length as usize + tag_length as usize;
            let mut in_out = vec![0u8; total_len];
            if input_data_length > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(input_data, in_out.as_mut_ptr(), input_data_length as usize);
                }
            }
            unsafe {
                ptr::copy_nonoverlapping(tag, in_out.as_mut_ptr().add(input_data_length as usize), tag_length as usize);
            }

            match less_safe_key.open_in_place(nonce, Aad::empty(), &mut in_out) {
                Ok(decrypted) => {
                    let decrypted_len = decrypted.len();
                    if decrypted_len > 0 {
                        unsafe {
                            ptr::copy_nonoverlapping(decrypted.as_ptr(), output_data, decrypted_len);
                        }
                    }
                    unsafe {
                        *output_data_length = decrypted_len as i32;
                    }
                    true
                }
                Err(_) => false,
            }
        }
        ALGORITHM_AES_CBC => {
            // AES-CBC decryption
            if key_length != 16 || iv_length != 16 {
                error!("PltDecryptMessage: invalid key/iv length for AES-CBC");
                return false;
            }

            if input_data_length <= 0 {
                unsafe { *output_data_length = 0; }
                return true;
            }

            // Input must be multiple of 16 bytes for AES-CBC
            if input_data_length % 16 != 0 {
                error!("PltDecryptMessage: input length not multiple of 16 for AES-CBC");
                return false;
            }

            let key_slice = unsafe { std::slice::from_raw_parts(key, key_length as usize) };
            let iv_slice = unsafe { std::slice::from_raw_parts(iv, iv_length as usize) };
            let input_len = input_data_length as usize;

            // Create decryptor
            let decryptor = match Aes128CbcDec::new_from_slices(key_slice, iv_slice) {
                Ok(dec) => dec,
                Err(_) => {
                    error!("PltDecryptMessage: failed to create AES-CBC decryptor");
                    return false;
                }
            };

            // Decrypt directly from input to output buffer
            let input_slice = unsafe { std::slice::from_raw_parts(input_data, input_len) };
            let output_slice = unsafe { std::slice::from_raw_parts_mut(output_data, input_len) };

            // Copy input to output for in-place decryption
            output_slice.copy_from_slice(input_slice);

            // When CIPHER_FLAG_FINISH is set, use PKCS7 unpadding (this is what OpenSSL does by default)
            // This is critical for audio decryption where the Opus packets are PKCS7 padded
            if (flags & CIPHER_FLAG_FINISH) != 0 {
                match decryptor.decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(output_slice) {
                    Ok(decrypted) => {
                        unsafe {
                            *output_data_length = decrypted.len() as i32;
                        }
                        true
                    }
                    Err(e) => {
                        error!("PltDecryptMessage: AES-CBC decryption with PKCS7 unpadding failed: {:?}", e);
                        false
                    }
                }
            } else {
                // No padding removal - decrypt as-is
                match decryptor.decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(output_slice) {
                    Ok(decrypted) => {
                        unsafe {
                            *output_data_length = decrypted.len() as i32;
                        }
                        true
                    }
                    Err(e) => {
                        error!("PltDecryptMessage: AES-CBC decryption failed: {:?}", e);
                        false
                    }
                }
            }
        }
        _ => false,
    }
}

/// Generate random data
#[no_mangle]
pub extern "C" fn PltGenerateRandomData(data: *mut u8, length: i32) {
    if data.is_null() || length <= 0 {
        return;
    }

    let buffer_slice = unsafe { std::slice::from_raw_parts_mut(data, length as usize) };

    if ring::rand::SystemRandom::new().fill(buffer_slice).is_err() {
        // If random generation fails, zero the buffer as a fallback
        buffer_slice.fill(0);
    }
}

/// Create HMAC-SHA256 context
#[no_mangle]
pub extern "C" fn PltCreateHmacSha256Context(key: *const u8, key_len: i32) -> *mut std::ffi::c_void {
    if key.is_null() || key_len <= 0 {
        return ptr::null_mut();
    }

    let key_slice = unsafe { std::slice::from_raw_parts(key, key_len as usize) };
    let hmac_key = hmac::Key::new(hmac::HMAC_SHA256, key_slice);

    let context = Box::new(HmacContext { key: hmac_key });
    Box::into_raw(context) as *mut std::ffi::c_void
}

/// Destroy HMAC context
#[no_mangle]
pub extern "C" fn PltDestroyHmacSha256Context(ctx: *mut std::ffi::c_void) {
    if !ctx.is_null() {
        unsafe {
            drop(Box::from_raw(ctx as *mut HmacContext));
        }
    }
}

/// HMAC-SHA256 sign
#[no_mangle]
#[inline]
pub extern "C" fn PltSignHmacSha256(
    ctx: *mut std::ffi::c_void,
    data: *const u8,
    data_len: i32,
    signature: *mut u8,
    signature_len: *mut i32,
) -> i32 {
    if ctx.is_null() || (data_len > 0 && data.is_null()) || signature.is_null() || signature_len.is_null() {
        return -1;
    }

    let context = unsafe { &*(ctx as *const HmacContext) };
    let data_slice = if data_len > 0 {
        unsafe { std::slice::from_raw_parts(data, data_len as usize) }
    } else {
        &[]
    };

    let tag = hmac::sign(&context.key, data_slice);
    let tag_bytes = tag.as_ref();

    unsafe {
        *signature_len = tag_bytes.len() as i32;
        ptr::copy_nonoverlapping(tag_bytes.as_ptr(), signature, tag_bytes.len());
    }

    0
}

/// SHA-256 hash
#[no_mangle]
pub extern "C" fn PltSha256(
    data: *const u8,
    data_len: i32,
    hash: *mut u8,
) -> i32 {
    if (data_len > 0 && data.is_null()) || hash.is_null() {
        return -1;
    }

    let data_slice = if data_len > 0 {
        unsafe { std::slice::from_raw_parts(data, data_len as usize) }
    } else {
        &[]
    };

    let digest_value = digest(&SHA256, data_slice);
    let digest_bytes = digest_value.as_ref();

    unsafe {
        ptr::copy_nonoverlapping(digest_bytes.as_ptr(), hash, digest_bytes.len());
    }

    0
}

/// Generate random bytes
#[no_mangle]
pub extern "C" fn PltGenerateRandomBytes(
    buffer: *mut u8,
    size: i32,
) -> i32 {
    if buffer.is_null() || size <= 0 {
        return -1;
    }

    let buffer_slice = unsafe { std::slice::from_raw_parts_mut(buffer, size as usize) };

    match ring::rand::SystemRandom::new().fill(buffer_slice) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}
