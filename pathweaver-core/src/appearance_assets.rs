//! Portable, named copies of appearance graphs. Applying a set never links its users.
use super::appearance::Graph;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub category: String,
    pub graph: Graph,
}
impl Preset {
    pub fn new(name: &str, category: &str, graph: &Graph) -> Result<Self, String> {
        let preset = Self {
            name: name.trim().into(),
            category: category.trim().into(),
            graph: graph.clone(),
        };
        preset.validate()?;
        Ok(preset)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.chars().count() > 80
            || self.category.chars().count() > 40
        {
            return Err("セット名は1〜80文字、種類は40文字以内で指定してください".into());
        }
        self.graph.validate()
    }
    pub fn apply(&self) -> Graph {
        let mut graph = self.graph.clone();
        graph.name = self.name.clone();
        graph
    }
    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(&File {
            format: "pathweaver-nodes".into(),
            version: 1,
            preset: self.clone(),
        })
        .map_err(|e| e.to_string())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("ノードセットは2MBまでです".into());
        }
        let file: File = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if file.format != "pathweaver-nodes" || file.version != 1 {
            return Err("未対応のノードセット形式です".into());
        }
        file.preset.validate()?;
        Ok(file.preset)
    }
}
#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    preset: Preset,
}

pub fn register(library: &mut Vec<Preset>, preset: Preset) -> Result<(), String> {
    preset.validate()?;
    if let Some(existing) = library
        .iter_mut()
        .find(|p| p.name == preset.name && p.category == preset.category)
    {
        *existing = preset;
    } else {
        if library.len() >= 32 {
            return Err("文書内のノードセットは32個までです".into());
        }
        library.push(preset);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_sets_roundtrip_and_apply_as_independent_copies() {
        let mut graph = Graph::rough();
        graph.node_mut(3).unwrap().label = "紙のふち".into();
        let set = Preset::new("紙の質感", "文字", &graph).unwrap();
        let saved = Preset::decode(set.encode().unwrap().as_bytes()).unwrap();
        assert_eq!(saved, set);
        let mut first = saved.apply();
        let second = saved.apply();
        first.node_mut(3).unwrap().amount = 40.;
        assert_eq!(second.node(3).unwrap().amount, 3.);
        assert_eq!(set.graph.node(3).unwrap().amount, 3.);
        let mut library = vec![];
        register(&mut library, set.clone()).unwrap();
        register(
            &mut library,
            Preset::new("紙の質感", "文字", &first).unwrap(),
        )
        .unwrap();
        assert_eq!(library.len(), 1);
        assert_eq!(library[0].graph, first);
        let mut value: serde_json::Value = serde_json::from_str(&set.encode().unwrap()).unwrap();
        value["version"] = 999.into();
        assert!(Preset::decode(value.to_string().as_bytes()).is_err());
        assert!(Preset::new(" ", "", &graph).is_err());
    }
}
