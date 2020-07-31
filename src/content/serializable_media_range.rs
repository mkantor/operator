use mime::Mime;
use serde::{Serialize, Serializer};

#[derive(Clone, PartialEq, Eq)]
pub struct SerializableMediaRange<'a> {
    media_type: &'a Mime,
}

impl<'a> From<&'a Mime> for SerializableMediaRange<'a> {
    fn from(media_type: &'a Mime) -> Self {
        SerializableMediaRange { media_type }
    }
}

impl<'a> Serialize for SerializableMediaRange<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.media_type.essence_str())
    }
}

impl<'a> PartialEq<Mime> for SerializableMediaRange<'a> {
    fn eq(&self, other: &Mime) -> bool {
        self.media_type == other
    }
}

impl<'a> PartialEq<SerializableMediaRange<'a>> for Mime {
    fn eq(&self, other: &SerializableMediaRange) -> bool {
        self == other.media_type
    }
}
