use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
enum OsEnv {
    Persisted(Vec<(String, String)>),
    Fresh(Vec<(String, String)>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessEnv {
    #[serde(
        skip_serializing_if = "OsEnv::should_not_persist",
        default = "OsEnv::empty"
    )]
    from_env: OsEnv,
    from_kv: Vec<(String, String)>,
    masked: Vec<String>,
}

impl OsEnv {
    fn new(persisted: bool) -> Self {
        let env = std::env::vars().collect();
        if persisted {
            Self::Persisted(env)
        } else {
            Self::Fresh(env)
        }
    }

    fn empty() -> Self {
        Self::Fresh(std::env::vars().collect())
    }

    fn should_not_persist(&self) -> bool {
        matches!(self, OsEnv::Fresh(_))
    }
}

impl IntoIterator for OsEnv {
    type Item = (String, String);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Persisted(p) => p.into_iter(),
            Self::Fresh(p) => p.into_iter(),
        }
    }
}

impl ProcessEnv {
    pub fn new(from_kv: Vec<(String, String)>, masked: Vec<String>, snapshot_env: bool) -> Self {
        Self {
            from_env: OsEnv::new(snapshot_env),
            from_kv,
            masked,
        }
    }

    pub fn from_reader<R: std::io::Read>(rdr: R) -> serde_json::Result<Self> {
        serde_json::from_reader(rdr)
    }

    pub fn to_writer<W: std::io::Write>(&self, w: W) -> serde_json::Result<()> {
        serde_json::to_writer(w, self)
    }

    pub fn into_env(self) -> HashMap<String, String> {
        let mut map: HashMap<_, _> = self.from_env.into_iter().collect();
        map.extend(self.from_kv);
        for m in self.masked {
            map.remove(&m);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ProcessEnv {
        pub fn fresh(
            from_env: Vec<(String, String)>,
            from_kv: Vec<(String, String)>,
            masked: Vec<String>,
        ) -> Self {
            Self {
                from_env: OsEnv::Fresh(from_env),
                from_kv,
                masked,
            }
        }

        pub fn from_str(s: &str) -> Self {
            serde_json::from_str(s).unwrap()
        }

        pub fn to_string(&self) -> String {
            serde_json::to_string(self).unwrap()
        }
    }

    macro_rules! env {
        ($a:expr) => {
            $a.to_string()
        };
        ($a:expr, $b:expr) => {
            ($a.to_string(), $b.to_string())
        };
    }

    #[test]
    fn into_env() {
        let env = ProcessEnv {
            from_env: OsEnv::Persisted(vec![env!("A", "ENV"), env!("B", "ENV"), env!("C", "ENV")]),
            from_kv: vec![
                env!("A", "KV"),
                env!("B", "KV"),
                env!("D", "KV"),
                env!("E", "KV"),
            ],
            masked: vec![env!("B"), env!("E")],
        };

        let env = env.into_env();

        assert_eq!(Some(&env!("KV")), env.get("A"));
        assert_eq!(None, env.get("B"));
        assert_eq!(Some(&env!("ENV")), env.get("C"));
        assert_eq!(Some(&env!("KV")), env.get("D"));
        assert_eq!(None, env.get("E"));
    }

    #[test]
    fn serialization_persisted() {
        let persisted = |env, kv, masked| ProcessEnv {
            from_env: OsEnv::Persisted(env),
            from_kv: kv,
            masked,
        };

        let test = |env: &ProcessEnv| {
            let serialized = env.to_string();
            ProcessEnv::from_str(&serialized)
        };

        let env = vec![env!("A", "B")];
        let kv = vec![env!("C", "D")];
        let masked = vec![env!("E")];
        let proc_env = persisted(env.clone(), kv.clone(), masked.clone());

        let serialized = test(&proc_env);

        assert_eq!(masked, serialized.masked);
        assert_eq!(kv, serialized.from_kv);
        assert!(matches!(serialized.from_env, OsEnv::Persisted(_)));
        assert_eq!(env, serialized.from_env.into_iter().collect::<Vec<_>>());
    }

    #[test]
    fn serialization_fresh() {
        let fresh = |kv, masked| ProcessEnv {
            from_env: OsEnv::Fresh(vec![env!("Ignore", "me")]),
            from_kv: kv,
            masked,
        };

        let test = |env: &ProcessEnv| {
            let serialized = env.to_string();
            ProcessEnv::from_str(&serialized)
        };

        let kv = vec![env!("C", "D")];
        let masked = vec![env!("E")];
        let proc_env = fresh(kv.clone(), masked.clone());

        let serialized = test(&proc_env);

        assert_eq!(masked, serialized.masked);
        assert_eq!(kv, serialized.from_kv);
        assert!(matches!(serialized.from_env, OsEnv::Fresh(_)));
        assert_eq!(
            std::env::vars().collect::<Vec<_>>(),
            serialized.from_env.into_iter().collect::<Vec<_>>()
        );
    }
}
