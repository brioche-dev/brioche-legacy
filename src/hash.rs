use std::{
    fmt::{Debug, Display},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

const SHA256_DIGEST_SIZE: usize = 32;

#[serde_with::serde_as]
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hash {
    #[serde_as(as = "serde_with::hex::Hex")]
    bytes: [u8; SHA256_DIGEST_SIZE],
}

impl Hash {
    pub fn from_bytes(bytes: [u8; SHA256_DIGEST_SIZE]) -> Self {
        Self { bytes }
    }

    pub fn from_digest(digest: sha2::Sha256) -> Self {
        use sha2::Digest as _;
        let bytes = digest.finalize();
        let bytes: [u8; SHA256_DIGEST_SIZE] = bytes
            .try_into()
            .expect("Could not convert SHA256 digest to expected size");
        Self { bytes }
    }

    pub fn to_path_component(&self) -> PathBuf {
        PathBuf::from(self.to_string())
    }
}

impl Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hash({})", self)
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO: Write without using a temporary buffer. See:
        // https://github.com/KokaKiwi/rust-hex/issues/54
        let mut hex_bytes = [0u8; SHA256_DIGEST_SIZE * 2];
        hex::encode_to_slice(self.bytes, &mut hex_bytes).expect("Failed to write hex string");
        let hex_str =
            std::str::from_utf8(&hex_bytes).expect("Failed to decode hex string as UTF-8");
        f.write_str(hex_str)
    }
}
