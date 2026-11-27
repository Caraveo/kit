//! The `Save` module: values that last between plays and game launches
//! (Unity's PlayerPrefs). Stored as JSON in the user's data folder, one file
//! per project: `<data dir>/Kit/saves/<project>.json`.

use kit_ecs::World;
use kit_resource::Resources;
use kit_x::Value;
use serde_json::{Map, Value as Json};
use std::path::PathBuf;

/// Loaded save data (a world resource, created on first use).
#[derive(Default)]
pub struct SaveData {
    path: Option<PathBuf>,
    data: Map<String, Json>,
    loaded: bool,
}

/// Where saves live on this computer.
fn data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "windows") {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
    }
}

/// The save file for the open project (named after its folder).
fn save_path(world: &World) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KIT_SAVE_FILE") {
        return Some(PathBuf::from(p));
    }
    let res = world.try_resource::<Resources>()?;
    let name = res.root().file_name()?.to_string_lossy().into_owned();
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    Some(data_dir()?.join("Kit").join("saves").join(format!("{safe}.json")))
}

fn to_json(v: &Value) -> Result<Json, String> {
    Ok(match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f).map(Json::Number).ok_or("can't save NaN or infinity")?,
        Value::Str(s) => Json::String(s.to_string()),
        Value::Vector(v) => serde_json::json!({ "$vector": v }),
        Value::List(l) => Json::Array(l.borrow().iter().map(to_json).collect::<Result<_, _>>()?),
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, v) in o.borrow().iter() {
                m.insert(k.to_string(), to_json(v)?);
            }
            Json::Object(m)
        }
        other => return Err(format!("a {} can't be saved; save numbers, text, vectors, lists or objects", other.type_name())),
    })
}

fn from_json(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n.as_i64().map(Value::Int).unwrap_or_else(|| Value::Float(n.as_f64().unwrap_or(0.0))),
        Json::String(s) => Value::Str(s.as_str().into()),
        Json::Array(a) => Value::list(a.iter().map(from_json).collect()),
        Json::Object(m) => match m.get("$vector").and_then(|v| v.as_array()) {
            Some(v) if m.len() == 1 && v.len() == 3 => {
                Value::vector(v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0), v[2].as_f64().unwrap_or(0.0))
            }
            _ => Value::object(m.iter().map(|(k, v)| (k.as_str(), from_json(v))).collect()),
        },
    }
}

fn load(world: &World) {
    if world.try_resource::<SaveData>().is_some_and(|s| s.loaded) {
        return;
    }
    let path = save_path(world);
    let data = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Map<String, Json>>(&t).ok())
        .unwrap_or_default();
    if let Some(mut s) = world.try_resource_mut::<SaveData>() {
        *s = SaveData { path, data, loaded: true };
    }
}

fn write(s: &SaveData) -> Result<(), String> {
    let Some(path) = &s.path else { return Err("saving isn't available here".into()) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't save: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&s.data).map_err(|e| e.to_string())?;
    // Write then rename, so a crash never leaves half a save.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("can't save: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("can't save: {e}"))
}

/// `Save.Set(key, value)`, `Get(key[, default])`, `Has(key)`, `Delete(key)`, `Clear()`, `Keys()`.
pub fn member(world: &mut World, member: &str, args: &[Value]) -> Result<Value, String> {
    if world.try_resource::<SaveData>().is_none() {
        world.insert_resource(SaveData::default());
    }
    load(world);
    let key = || args.first().and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| format!("{member}(\"key\", ...)"));
    let mut s = world.resource_mut::<SaveData>();
    match member {
        "Set" => {
            let k = key()?;
            let v = to_json(args.get(1).ok_or("Set(\"key\", value)")?)?;
            s.data.insert(k, v);
            write(&s)?;
            Ok(Value::Null)
        }
        "Get" => Ok(s.data.get(&key()?).map(from_json).unwrap_or_else(|| args.get(1).cloned().unwrap_or(Value::Null))),
        "Has" => Ok(Value::Bool(s.data.contains_key(&key()?))),
        "Delete" => {
            s.data.remove(&key()?);
            write(&s)?;
            Ok(Value::Null)
        }
        "Clear" => {
            s.data.clear();
            write(&s)?;
            Ok(Value::Null)
        }
        "Keys" => Ok(Value::list(s.data.keys().map(|k| Value::Str(k.as_str().into())).collect())),
        _ => Err(format!("unknown member `{member}`")),
    }
}
