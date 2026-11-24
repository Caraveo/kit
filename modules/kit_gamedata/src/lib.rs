//! # kit_gamedata
//!
//! GameData: the designer-facing data of a game, kept apart from code.
//!
//! * **Datasets**: named tables with typed **fields** (columns) and named
//!   **rows** ("Room Presets" → row "Low Gravity Workshop" → field "Tint").
//! * **Constants**: standalone named values ("Max Health").
//!
//! Datasets, fields, rows and constants have **stable IDs** separate from
//! their names, so renaming never breaks a Diagram graph that uses them.
//! Values are typed ([`ValueType`]); Int and Float stay distinct.
//!
//! The data lives in one project file, [`FILE`] (`game.kdata`, JSON), loaded
//! into the [`GameData`] resource. It's read-only while the game plays;
//! graphs keep mutable state in variables instead.

use anyhow::{Context, Result};
use kit_core::{App, Module};
use serde::{Deserialize, Serialize};

/// The project's GameData file.
pub const FILE: &str = "game.kdata";

/// A stable identifier ("ds_7f3a…", "f_…", "r_…", "c_…").
pub type Id = String;

/// Types a field, constant, variable or graph pin can hold.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    Bool,
    Int,
    Float,
    Text,
    Color,
    Vector2,
    Vector3,
    Transform,
    /// A live scene object.
    Entity,
    /// A row of the dataset with this ID.
    Row(Id),
}

impl ValueType {
    /// The basic types a dataset field or constant can be.
    pub const FIELD_TYPES: [ValueType; 8] = [
        ValueType::Bool,
        ValueType::Int,
        ValueType::Float,
        ValueType::Text,
        ValueType::Color,
        ValueType::Vector2,
        ValueType::Vector3,
        ValueType::Transform,
    ];

    pub fn label(&self) -> String {
        match self {
            ValueType::Bool => "Bool".into(),
            ValueType::Int => "Int".into(),
            ValueType::Float => "Float".into(),
            ValueType::Text => "Text".into(),
            ValueType::Color => "Color".into(),
            ValueType::Vector2 => "Vector2".into(),
            ValueType::Vector3 => "Vector3".into(),
            ValueType::Transform => "Transform".into(),
            ValueType::Entity => "Object".into(),
            ValueType::Row(_) => "Row".into(),
        }
    }

    /// The value a new field/variable of this type starts with.
    pub fn default_value(&self) -> Value {
        match self {
            ValueType::Bool => Value::Bool(false),
            ValueType::Int => Value::Int(0),
            ValueType::Float => Value::Float(0.0),
            ValueType::Text => Value::Text(String::new()),
            ValueType::Color => Value::Color([1.0, 1.0, 1.0, 1.0]),
            ValueType::Vector2 => Value::Vector2([0.0; 2]),
            ValueType::Vector3 => Value::Vector3([0.0; 3]),
            ValueType::Transform => Value::Transform(TransformValue::default()),
            ValueType::Entity => Value::Entity(None),
            ValueType::Row(ds) => Value::Row { dataset: ds.clone(), row: None },
        }
    }
}

/// Position, rotation (Euler degrees) and scale, as authored.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransformValue {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for TransformValue {
    fn default() -> Self {
        Self { position: [0.0; 3], rotation: [0.0; 3], scale: [1.0; 3] }
    }
}

/// A typed value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// sRGB, with alpha.
    Color([f32; 4]),
    Vector2([f32; 2]),
    Vector3([f32; 3]),
    Transform(TransformValue),
    /// A live object at runtime (entity bits); `None` = nothing.
    Entity(Option<u64>),
    Row {
        dataset: Id,
        row: Option<Id>,
    },
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Bool(_) => ValueType::Bool,
            Value::Int(_) => ValueType::Int,
            Value::Float(_) => ValueType::Float,
            Value::Text(_) => ValueType::Text,
            Value::Color(_) => ValueType::Color,
            Value::Vector2(_) => ValueType::Vector2,
            Value::Vector3(_) => ValueType::Vector3,
            Value::Transform(_) => ValueType::Transform,
            Value::Entity(_) => ValueType::Entity,
            Value::Row { dataset, .. } => ValueType::Row(dataset.clone()),
        }
    }

    /// Short readable form (debug overlays, the inspector).
    pub fn display(&self) -> String {
        let f = |x: f32| format!("{x:.2}");
        match self {
            Value::Bool(b) => if *b { "true" } else { "false" }.into(),
            Value::Int(i) => i.to_string(),
            Value::Float(x) => format!("{x:.3}"),
            Value::Text(t) => format!("\"{t}\""),
            Value::Color(c) => {
                format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
            }
            Value::Vector2(v) => format!("({}, {})", f(v[0]), f(v[1])),
            Value::Vector3(v) => format!("({}, {}, {})", f(v[0]), f(v[1]), f(v[2])),
            Value::Transform(t) => format!("at ({}, {}, {})", f(t.position[0]), f(t.position[1]), f(t.position[2])),
            Value::Entity(e) => {
                if e.is_some() {
                    "object".into()
                } else {
                    "none".into()
                }
            }
            Value::Row { row, .. } => row.clone().unwrap_or_else(|| "no row".into()),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(x) => Some(*x),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: Id,
    pub name: String,
    pub value_type: ValueType,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: Id,
    pub name: String,
    /// Field id → value. Missing fields read as the field type's default.
    #[serde(default)]
    pub values: std::collections::BTreeMap<Id, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dataset {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub rows: Vec<Row>,
}

impl Dataset {
    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }
    /// A row's value for a field (the field's default when unset or the
    /// stored value no longer matches the field's type).
    pub fn value(&self, row: &str, field: &str) -> Option<Value> {
        let f = self.field(field)?;
        let r = self.row(row)?;
        Some(
            r.values
                .get(field)
                .filter(|v| v.value_type() == f.value_type)
                .cloned()
                .unwrap_or_else(|| f.value_type.default_value()),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constant {
    pub id: Id,
    pub name: String,
    pub value: Value,
}

/// All of a project's GameData (a world resource; [`FILE`] on disk).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameData {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub datasets: Vec<Dataset>,
    #[serde(default)]
    pub constants: Vec<Constant>,
    /// Counter for new IDs (with a random part so copies don't collide).
    #[serde(default)]
    pub next_id: u64,
    /// Bumped on every edit, so editors and graph caches can notice changes.
    #[serde(skip)]
    pub revision: u64,
}

fn version() -> u32 {
    1
}

impl GameData {
    pub fn load(resources: &kit_resource::Resources) -> Result<Self> {
        if !resources.exists(FILE) {
            return Ok(Self { version: 1, ..Default::default() });
        }
        let text = resources.read_text(FILE)?;
        let mut data: Self = serde_json::from_str(&text).with_context(|| format!("reading {FILE}"))?;
        data.version = data.version.max(1);
        Ok(data)
    }

    pub fn save(&self, resources: &kit_resource::Resources) -> Result<()> {
        resources.write_text(FILE, &(serde_json::to_string_pretty(self)? + "\n"))
    }

    /// A new stable ID with a prefix (`ds`, `f`, `r`, `c`).
    pub fn new_id(&mut self, prefix: &str) -> Id {
        self.next_id += 1;
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        format!("{prefix}_{:x}{:04x}", self.next_id, (salt ^ (self.next_id * 0x9E37)) & 0xffff)
    }

    pub fn dataset(&self, id: &str) -> Option<&Dataset> {
        self.datasets.iter().find(|d| d.id == id)
    }
    pub fn dataset_mut(&mut self, id: &str) -> Option<&mut Dataset> {
        self.revision += 1;
        self.datasets.iter_mut().find(|d| d.id == id)
    }
    pub fn dataset_by_name(&self, name: &str) -> Option<&Dataset> {
        self.datasets.iter().find(|d| d.name == name)
    }
    pub fn constant(&self, id: &str) -> Option<&Constant> {
        self.constants.iter().find(|c| c.id == id)
    }

    // Editing (the GameData view). Each bumps `revision`.

    pub fn add_dataset(&mut self, name: &str) -> Id {
        let id = self.new_id("ds");
        self.datasets.push(Dataset { id: id.clone(), name: name.into(), fields: Vec::new(), rows: Vec::new() });
        self.revision += 1;
        id
    }

    pub fn add_field(&mut self, dataset: &str, name: &str, value_type: ValueType) -> Option<Id> {
        let id = self.new_id("f");
        let ds = self.dataset_mut(dataset)?;
        ds.fields.push(Field { id: id.clone(), name: name.into(), value_type });
        Some(id)
    }

    pub fn add_row(&mut self, dataset: &str, name: &str) -> Option<Id> {
        let id = self.new_id("r");
        let ds = self.dataset_mut(dataset)?;
        ds.rows.push(Row { id: id.clone(), name: name.into(), values: Default::default() });
        Some(id)
    }

    pub fn set_value(&mut self, dataset: &str, row: &str, field: &str, value: Value) -> bool {
        let Some(ds) = self.dataset_mut(dataset) else { return false };
        let Some(ty) = ds.field(field).map(|f| f.value_type.clone()) else { return false };
        if value.value_type() != ty {
            return false;
        }
        match ds.rows.iter_mut().find(|r| r.id == row) {
            Some(r) => {
                r.values.insert(field.to_owned(), value);
                true
            }
            None => false,
        }
    }

    pub fn add_constant(&mut self, name: &str, value: Value) -> Id {
        let id = self.new_id("c");
        self.constants.push(Constant { id: id.clone(), name: name.into(), value });
        self.revision += 1;
        id
    }
}

/// Loads the project's GameData into the [`GameData`] resource.
pub struct GameDataModule;

impl Module for GameDataModule {
    fn name(&self) -> &'static str {
        "gamedata"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["resource"]
    }
    fn build(&self, app: &mut App) {
        let data = match app.world.try_resource::<kit_resource::Resources>() {
            Some(r) => GameData::load(&r).unwrap_or_else(|e| {
                log::error!("{e:#}");
                GameData::default()
            }),
            None => GameData::default(),
        };
        app.world.insert_resource(data);
    }
}

/// Reload GameData from the project (after the resources change).
pub fn reload(world: &mut kit_ecs::World) {
    let loaded = world.try_resource::<kit_resource::Resources>().map(|r| GameData::load(&r));
    match loaded {
        Some(Ok(d)) => world.insert_resource(d),
        Some(Err(e)) => log::error!("{e:#}"),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datasets_rows_fields_and_renames() {
        let mut gd = GameData::default();
        let ds = gd.add_dataset("Room Presets");
        let tint = gd.add_field(&ds, "Tint", ValueType::Color).unwrap();
        let grav = gd.add_field(&ds, "Gravity Scale", ValueType::Float).unwrap();
        let row = gd.add_row(&ds, "Low Gravity Workshop").unwrap();
        assert!(gd.set_value(&ds, &row, &tint, Value::Color([0.2, 0.8, 1.0, 1.0])));
        assert!(!gd.set_value(&ds, &row, &grav, Value::Int(2)), "Int is not Float");
        assert!(gd.set_value(&ds, &row, &grav, Value::Float(0.3)));
        // Renames keep IDs.
        gd.dataset_mut(&ds).unwrap().name = "Rooms".into();
        gd.dataset_mut(&ds).unwrap().fields[0].name = "Color".into();
        assert_eq!(gd.dataset(&ds).unwrap().value(&row, &tint), Some(Value::Color([0.2, 0.8, 1.0, 1.0])));
        // A type change: stored values of the old type read as the new default.
        gd.dataset_mut(&ds).unwrap().fields[1].value_type = ValueType::Int;
        assert_eq!(gd.dataset(&ds).unwrap().value(&row, &grav), Some(Value::Int(0)));
        // IDs are unique.
        let ids: std::collections::HashSet<_> = [ds, tint, grav, row].into_iter().collect();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn round_trips_through_json() {
        let mut gd = GameData::default();
        let ds = gd.add_dataset("Items");
        let f = gd.add_field(&ds, "Spawn", ValueType::Transform).unwrap();
        let r = gd.add_row(&ds, "Sword").unwrap();
        gd.set_value(&ds, &r, &f, Value::Transform(TransformValue { position: [1.0, 2.0, 3.0], ..Default::default() }));
        gd.add_constant("Max Health", Value::Int(100));
        let text = serde_json::to_string(&gd).unwrap();
        let back: GameData = serde_json::from_str(&text).unwrap();
        assert_eq!(back.datasets, gd.datasets);
        assert_eq!(back.constants, gd.constants);
        // Older/empty files still load.
        let empty: GameData = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.version, 1);
    }
}
