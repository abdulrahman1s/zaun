//! Enums with a stable string form that keep unknown values (`TextEncoding` + `X_Unknown Text`).

/// `open_enum!(Role { Admin = "admin" })` makes `enum Role { Admin, Unknown(String) }` with
/// `as_str`, `from_wire` and string serde.
macro_rules! open_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum $name {
            $($var,)+
            Unknown(String),
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$var => $s,)+
                    Self::Unknown(s) => s,
                }
            }

            pub fn from_wire(s: &str) -> Self {
                match s {
                    $($s => Self::$var,)+
                    other => Self::Unknown(other.to_owned()),
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Ok(Self::from_wire(&s))
            }
        }
    };
}

pub(crate) use open_enum;
