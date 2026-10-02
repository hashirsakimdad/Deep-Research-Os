//! Domain models shared by agents, the orchestrator, the database and the API.

pub mod claim;
pub mod report;
pub mod research;
pub mod source;

pub use claim::*;
pub use report::*;
pub use research::*;
pub use source::*;

/// Declares an enum that serializes as a fixed string, with `as_str` and
/// `FromStr` so the same spelling is used in JSON, SQL and LLM output.
macro_rules! string_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(&self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let s = s.trim();
                $(if s.eq_ignore_ascii_case($text) { return Ok(Self::$variant); })+
                Err(format!("unknown {} value {:?}", stringify!($name), s))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = <String as serde::Deserialize>::deserialize(d)?;
                raw.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}
pub(crate) use string_enum;
