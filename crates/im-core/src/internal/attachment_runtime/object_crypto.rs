use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;

pub(crate) const OBJECT_E2EE_CIPHER: &str = "chacha20-poly1305";
pub(crate) const OBJECT_E2EE_KEY_LEN: usize = 32;
pub(crate) const OBJECT_E2EE_NONCE_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectE2eeCryptoMaterial {
    pub object_cipher: String,
    pub object_key_b64u: String,
    pub nonce_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncryptedAttachmentObject {
    pub ciphertext: Vec<u8>,
    pub ciphertext_size_bytes: u64,
    pub ciphertext_size_string: String,
    pub ciphertext_digest_b64u: String,
    pub plaintext_size_bytes: u64,
    pub plaintext_size_string: String,
    pub crypto: ObjectE2eeCryptoMaterial,
}

pub(crate) fn encrypt_object_e2ee(plaintext: &[u8]) -> crate::ImResult<EncryptedAttachmentObject> {
    let mut object_key = [0_u8; OBJECT_E2EE_KEY_LEN];
    let mut nonce = [0_u8; OBJECT_E2EE_NONCE_LEN];
    OsRng.fill_bytes(&mut object_key);
    OsRng.fill_bytes(&mut nonce);

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&object_key));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &[],
            },
        )
        .map_err(|_| crate::ImError::Internal {
            message: "object-e2ee encryption failed".to_string(),
        })?;
    let ciphertext_size_bytes = ciphertext.len() as u64;
    let plaintext_size_bytes = plaintext.len() as u64;

    Ok(EncryptedAttachmentObject {
        ciphertext_digest_b64u: super::digest::sha256_digest_b64u(&ciphertext),
        ciphertext_size_string: ciphertext_size_bytes.to_string(),
        ciphertext_size_bytes,
        plaintext_size_string: plaintext_size_bytes.to_string(),
        plaintext_size_bytes,
        ciphertext,
        crypto: ObjectE2eeCryptoMaterial {
            object_cipher: OBJECT_E2EE_CIPHER.to_string(),
            object_key_b64u: b64u_encode(&object_key),
            nonce_b64u: b64u_encode(&nonce),
        },
    })
}

pub(crate) fn decrypt_object_e2ee(
    ciphertext: &[u8],
    object_key_b64u: &str,
    nonce_b64u: &str,
) -> crate::ImResult<Vec<u8>> {
    let object_key = decode_fixed_b64u("object_key_b64u", object_key_b64u, OBJECT_E2EE_KEY_LEN)?;
    let nonce = decode_fixed_b64u("nonce_b64u", nonce_b64u, OBJECT_E2EE_NONCE_LEN)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&object_key));
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: ciphertext,
                aad: &[],
            },
        )
        .map_err(|_| decrypt_failed())
}

fn decode_fixed_b64u(field: &str, value: &str, expected_len: usize) -> crate::ImResult<Vec<u8>> {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value.trim())
        .map_err(|_| {
            crate::ImError::invalid_input(
                Some(field.to_string()),
                format!("{field} must be base64url without padding"),
            )
        })?;
    if decoded.len() != expected_len {
        return Err(crate::ImError::invalid_input(
            Some(field.to_string()),
            format!("{field} must decode to {expected_len} bytes"),
        ));
    }
    Ok(decoded)
}

fn b64u_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn decrypt_failed() -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("anp.attachment.decrypt_failed".to_string()),
        message: "object-e2ee object decryption failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_object_crypto_roundtrip_uses_random_key_nonce_and_ciphertext_digest() {
        let first = encrypt_object_e2ee(b"hello object").expect("first encrypted object");
        let second = encrypt_object_e2ee(b"hello object").expect("second encrypted object");

        assert_eq!(first.crypto.object_cipher, OBJECT_E2EE_CIPHER);
        assert_eq!(first.plaintext_size_bytes, 12);
        assert_eq!(first.plaintext_size_string, "12");
        assert_ne!(first.ciphertext, b"hello object".to_vec());
        assert_ne!(first.ciphertext, second.ciphertext);
        assert_ne!(first.crypto.object_key_b64u, second.crypto.object_key_b64u);
        assert_ne!(first.crypto.nonce_b64u, second.crypto.nonce_b64u);
        assert_ne!(first.ciphertext_digest_b64u, second.ciphertext_digest_b64u);
        assert_eq!(first.ciphertext_size_bytes, first.ciphertext.len() as u64);
        assert_eq!(
            first.ciphertext_size_string,
            first.ciphertext.len().to_string()
        );

        let plaintext = decrypt_object_e2ee(
            &first.ciphertext,
            &first.crypto.object_key_b64u,
            &first.crypto.nonce_b64u,
        )
        .expect("object decrypts");
        assert_eq!(plaintext, b"hello object".to_vec());
    }

    #[test]
    fn attachment_object_crypto_rejects_wrong_key_and_nonce() {
        let encrypted = encrypt_object_e2ee(b"hello object").expect("encrypted object");
        let other = encrypt_object_e2ee(b"other object").expect("other encrypted object");

        let err = decrypt_object_e2ee(
            &encrypted.ciphertext,
            &other.crypto.object_key_b64u,
            &encrypted.crypto.nonce_b64u,
        )
        .expect_err("wrong key should fail");
        assert!(matches!(
            err,
            crate::ImError::Service { code: Some(code), .. }
                if code == "anp.attachment.decrypt_failed"
        ));

        let err = decrypt_object_e2ee(
            &encrypted.ciphertext,
            &encrypted.crypto.object_key_b64u,
            "bad-nonce",
        )
        .expect_err("invalid nonce should fail");
        assert!(matches!(
            err,
            crate::ImError::InvalidInput { field: Some(field), message }
                if field == "nonce_b64u" && message.contains("base64url")
        ));
    }
}
