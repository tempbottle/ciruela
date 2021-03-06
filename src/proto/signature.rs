use std::fmt;
use std::cmp::Ordering;

use base64;
use crypto::ed25519;
use serde::{Serialize, Serializer};
use serde::{Deserialize, Deserializer};
use serde::de::{Visitor, SeqAccess, Error};
use serde_cbor::ser::Serializer as Cbor;
use ssh_keys::{PrivateKey, PublicKey};
use hexlify::Hex;


// Note everything here, must be stable-serialized
pub enum Signature {
    SshEd25519([u8; 64]),
}

enum SignatureType {
    SshEd25519,
}

struct SignatureVisitor;
struct TypeVisitor;
struct Ed25519Visitor;
struct Ed25519([u8; 64]);

const TYPES: &'static [&'static str] = &[
    "ssh-ed25519",
    ];

pub struct SigData<'a> {
    pub path: &'a str,
    pub image: &'a [u8],
    pub timestamp: u64,
}

pub struct Bytes<'a>(&'a [u8]);

pub fn sign(src: SigData, keys: &[PrivateKey]) -> Vec<Signature> {
    let mut buf = Vec::with_capacity(100);
    (src.path, Bytes(src.image), src.timestamp)
        .serialize(&mut Cbor::new(&mut buf))
        .expect("Can always serialize signature data");
    let mut res = Vec::new();
    for key in keys {
        let signature = match *key {
            PrivateKey::Ed25519(ref bytes) => {
                Signature::SshEd25519(ed25519::signature(&buf[..], &bytes[..]))
            }
            _ => {
                // TODO(tailhook) propagate error somehow
                error!("Unimplemented signature kind {:?}", key);
                ::std::process::exit(101);
            }
        };
        res.push(signature);
    }
    info!("Image {}[{}] signed with {} keys",
        src.path, Hex(src.image), res.len());
    return res;
}

pub fn verify(src: &SigData, signature: &Signature, keys: &[PublicKey])
    -> bool
{
    let mut buf = Vec::with_capacity(100);
    (src.path, Bytes(src.image), src.timestamp)
        .serialize(&mut Cbor::new(&mut buf))
        .expect("Can always serialize signature data");
    keys.iter().any(|key| {
        match (key, signature) {
            (&PublicKey::Ed25519(ref key), &Signature::SshEd25519(ref sig))
            => {
                ed25519::verify(&buf[..], &key[..], &sig[..])
            }
            _ => {
                false
            }
        }
    })
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        use self::Signature::*;
        if serializer.is_human_readable() {
            match *self {
                SshEd25519(val) => {
                    format!("ssh-ed25519 {}", base64::encode(&val[..]))
                    .serialize(serializer)
                }

            }
        } else {
            match *self {
                SshEd25519(val) => (
                    "ssh-ed25519", Bytes(&val[..])
                    ).serialize(serializer)
            }
        }
    }
}

impl<'a> Serialize for Bytes<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        serializer.serialize_bytes(&self.0[..])
    }
}

impl<'a> Deserialize<'a> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'a>
    {
        deserializer.deserialize_seq(SignatureVisitor)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Signature::*;

        match *self {
            SshEd25519(val) => {
                write!(f, "SshEd25519({})", base64::encode(&val[..]))
            }
        }
    }
}

impl Clone for Signature {
    fn clone(&self) -> Signature {
        use self::Signature::*;

        match *self {
            SshEd25519(val) => SshEd25519(val),
        }
    }
}

impl<'a> Visitor<'a> for TypeVisitor {
    type Value = SignatureType;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(&TYPES.join(", "))
    }

    fn visit_str<E>(self, value: &str) -> Result<SignatureType, E>
        where E: Error
    {
        match value {
            "ssh-ed25519" => Ok(SignatureType::SshEd25519),
            _ => Err(Error::unknown_field(value, TYPES)),
        }
    }
}

impl<'a> Visitor<'a> for Ed25519Visitor {
    type Value = Ed25519;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("64 bytes")
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Ed25519, E>
        where E: Error
    {
        if value.len() != 64 {
            return Err(Error::invalid_length(1, &self));
        }
        let mut res = [0u8; 64];
        res.copy_from_slice(value);
        return Ok(Ed25519(res));
    }
}

impl<'a> Deserialize<'a> for SignatureType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'a>
    {
        deserializer.deserialize_str(TypeVisitor)
    }
}

impl<'a> Deserialize<'a> for Ed25519 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'a>
    {
        deserializer.deserialize_bytes(Ed25519Visitor)
    }
}

impl<'a> Visitor<'a> for SignatureVisitor {
    type Value = Signature;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("signature")
    }

    #[inline]
    fn visit_seq<V>(self, mut visitor: V) -> Result<Signature, V::Error>
        where V: SeqAccess<'a>,
    {
        match visitor.next_element()? {
            Some(SignatureType::SshEd25519) => match visitor.next_element()? {
                Some(Ed25519(value)) => Ok(Signature::SshEd25519(value)),
                None => Err(Error::invalid_length(1, &self)),
            },
            None => Err(Error::custom("invalid signature type")),
        }
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Signature) -> bool {
        use self::Signature::*;
        match (self, other) {
            (&SshEd25519(ref x), &SshEd25519(ref y)) => &x[..] == &y[..],
        }
    }
}

impl Eq for Signature { }

impl PartialOrd for Signature {
    fn partial_cmp(&self, other: &Signature) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Signature {
    fn cmp(&self, other: &Signature) -> Ordering {
        use self::Signature::*;
        match (self, other) {
            (&SshEd25519(ref x), &SshEd25519(ref y)) => x[..].cmp(&y[..]),
        }
    }
}
