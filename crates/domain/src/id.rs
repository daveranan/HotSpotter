use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

stable_id!(ProjectId);
stable_id!(SourceId);
stable_id!(PatchId);
stable_id!(LayoutId);
stable_id!(LayerId);
stable_id!(MapId);

#[cfg(test)]
mod tests {
    use super::ProjectId;

    #[test]
    fn stable_ids_round_trip_through_text() {
        let id = ProjectId::new();
        let restored = id
            .to_string()
            .parse::<ProjectId>()
            .expect("valid project id");
        assert_eq!(id, restored);
    }
}
