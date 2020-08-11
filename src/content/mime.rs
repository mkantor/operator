use mime::Mime;
use serde::{Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

pub type MediaRange = Mime;

/// See [IETF RFC 2046](https://tools.ietf.org/html/rfc2046).
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct MediaType(MediaRange);
impl MediaType {
    pub const APPLICATION_OCTET_STREAM: Self = Self(mime::APPLICATION_OCTET_STREAM);

    pub fn from_media_range(media_range: MediaRange) -> Option<MediaType> {
        if media_range.type_() == "*" || media_range.subtype() == "*" {
            None
        } else {
            Some(MediaType(media_range))
        }
    }

    pub fn is_within_media_range(&self, media_range: &MediaRange) -> bool {
        if media_range == &::mime::STAR_STAR {
            true
        } else if media_range.subtype() == "*" {
            self.0.type_() == media_range.type_()
        } else {
            self == media_range
        }
    }

    pub fn into_media_range(self) -> MediaRange {
        self.0
    }
}

#[derive(Error, Debug)]
#[error("Could not parse media type: {}", .0)]
pub struct MediaTypeFromStrError(String);

impl FromStr for MediaType {
    type Err = MediaTypeFromStrError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mime = Mime::from_str(input)
            .map_err(|error| MediaTypeFromStrError(format!("Malformed media type: {}", error)))?;
        MediaType::from_media_range(mime).ok_or_else(|| {
            MediaTypeFromStrError(String::from(
                "Input is a valid media range, but not a specific media type",
            ))
        })
    }
}

impl fmt::Debug for MediaType {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, formatter)
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl Serialize for MediaType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl PartialEq<MediaRange> for MediaType {
    fn eq(&self, other: &MediaRange) -> bool {
        &self.0 == other
    }
}

impl PartialEq<MediaType> for MediaRange {
    fn eq(&self, other: &MediaType) -> bool {
        self == &other.0
    }
}
