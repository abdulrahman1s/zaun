//! The tree of a parsed description YAML.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Scalar(String),
    Map(Vec<(String, Node)>),
    Seq(Vec<Node>),
}

impl Node {
    /// The last value for `key`, as YAML mappings resolve duplicates.
    pub(super) fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Self::Map(m) => m.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            Self::Scalar(_) | Self::Seq(_) => None,
        }
    }

    pub(super) fn scalar(&self) -> Option<&str> {
        match self {
            Self::Scalar(s) => Some(s),
            Self::Map(_) | Self::Seq(_) => None,
        }
    }

    pub(super) fn items(&self) -> &[Node] {
        match self {
            Self::Seq(v) => v,
            Self::Scalar(_) | Self::Map(_) => &[],
        }
    }
}
