use mime::Mime;
use serde::{Serialize, Serializer};

#[derive(Clone, PartialEq, Eq)]
pub struct SerializableMediaRange {
    media_type: Mime,
}

impl From<Mime> for SerializableMediaRange {
    fn from(media_type: Mime) -> Self {
        SerializableMediaRange { media_type }
    }
}

impl Serialize for SerializableMediaRange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.media_type.essence_str())
    }
}

impl PartialEq<Mime> for SerializableMediaRange {
    fn eq(&self, other: &Mime) -> bool {
        &self.media_type == other
    }
}

impl PartialEq<SerializableMediaRange> for Mime {
    fn eq(&self, other: &SerializableMediaRange) -> bool {
        self == &other.media_type
    }
}
