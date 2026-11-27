//! Engine modules exposed to x scripts.

use glam::{Quat, Vec3, Vec4};
use kit_core::{Console, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{Model, Physics, Sprite};
use kit_input::Input;
use kit_resource::Resources;
use kit_scene::{GlobalTransform, Name, Transform};
use kit_x::{FeatureRef, Host, Value};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::States;

/// Modules scripts can `TAKE`.
pub const MODULES: &[&str] = &[
    "Transform", "Input", "Time", "Entity", "Scene", "Sprite", "Model", "Log", "Physics", "Collider", "Path", "Terrain",
    "Audio", "Diagram", "Light", "Camera", "Save", "Debug", "Interface", "Cinematic",
];

/// Engine features `GetFeature` / `AddFeature` know (besides scripts).
pub const ENGINE_FEATURES: &[&str] = &[
    "Transform", "Entity", "Model", "Sprite", "Physics", "Collider", "Audio", "Path", "Light", "Camera", "Terrain", "Diagram",
    "Particles",
];

/// Structural changes requested by scripts, applied after the script returns.
pub enum Command {
    Despawn(Entity),
    Scene(kit_gamesys::scenes::SceneOp),
    AddTimer(Timer),
    CancelTimer(u64),
    /// `Time.Start("Method", args...)`: run a method as a coroutine.
    StartCoroutine { id: u64, method: String, args: Vec<Value> },
    StopCoroutine(u64),
}

/// A pending `Time.After` / `Time.Every` call.
#[derive(Clone)]
pub struct Timer {
    pub id: u64,
    /// Game time (`Time.Elapsed`) when it fires.
    pub due: f64,
    /// Repeat interval for `Time.Every`.
    pub every: Option<f64>,
    pub method: String,
    pub args: Vec<Value>,
}

static NEXT_TIMER: AtomicU64 = AtomicU64::new(1);

pub struct EntityHost<'a> {
    world: &'a mut World,
    entity: Entity,
    res: Option<Resources>,
    commands: &'a mut Vec<Command>,
    /// The other scripts (this one is taken out while it runs).
    states: Option<&'a mut States>,
}

impl<'a> EntityHost<'a> {
    pub fn new(
        world: &'a mut World,
        entity: Entity,
        res: Option<Resources>,
        commands: &'a mut Vec<Command>,
        states: Option<&'a mut States>,
    ) -> Self {
        Self { world, entity, res, commands, states }
    }

    fn source(&self) -> String {
        name_of(self.world, self.entity)
    }
}

fn name_of(world: &World, e: Entity) -> String {
    world.get::<Name>(e).map(|n| n.0.clone()).unwrap_or_else(|| format!("{e:?}"))
}

/// The info a collision hook's second parameter receives:
/// `{ Point, Normal, Strength }`, the normal pointing away from the other object.
pub fn hit_value(ev: &kit_gamesys::CollisionEvent, sign: f32) -> Value {
    Value::object(vec![
        ("Point", vec_value(ev.point)),
        ("Normal", vec_value(-ev.normal * sign)),
        ("Strength", Value::Float(ev.strength as f64)),
    ])
}

type R = Result<Value, String>;

fn f(v: &Value, what: &str) -> Result<f32, String> {
    v.as_f64().map(|x| x as f32).ok_or_else(|| format!("{what} must be a number, got {}", v.type_name()))
}

/// Accepts `(VECTOR)` or `(x, y, z)`; with `two_d`, `(x, y)` too.
fn vec3_args(args: &[Value]) -> Result<Vec3, String> {
    match args {
        [Value::Vector(v)] => Ok(Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)),
        [x, y] => Ok(Vec3::new(f(x, "x")?, f(y, "y")?, 0.0)),
        [x, y, z] => Ok(Vec3::new(f(x, "x")?, f(y, "y")?, f(z, "z")?)),
        _ => Err("expected a VECTOR or (x, y, z)".into()),
    }
}

fn vec_value(v: Vec3) -> Value {
    Value::Vector([v.x as f64, v.y as f64, v.z as f64])
}

fn color_args(args: &[Value]) -> Result<Vec4, String> {
    match args {
        [Value::Vector(v)] => Ok(Vec4::new(v[0] as f32, v[1] as f32, v[2] as f32, 1.0)),
        [Value::Vector(v), a] => Ok(Vec4::new(v[0] as f32, v[1] as f32, v[2] as f32, f(a, "alpha")?)),
        [r, g, b] => Ok(Vec4::new(f(r, "r")?, f(g, "g")?, f(b, "b")?, 1.0)),
        [r, g, b, a] => Ok(Vec4::new(f(r, "r")?, f(g, "g")?, f(b, "b")?, f(a, "a")?)),
        _ => Err("expected (r, g, b), (r, g, b, a) or a VECTOR".into()),
    }
}

fn no_args(args: &[Value], member: &str) -> Result<(), String> {
    if args.is_empty() { Ok(()) } else { Err(format!("{member} takes no arguments")) }
}

fn entity_id(e: Entity) -> Value {
    Value::Int(e.to_bits() as i64)
}

fn arg_entity(world: &World, v: &Value) -> Result<Entity, String> {
    let id = v.as_i64().ok_or("expected an entity id (INTEGER)")?;
    let e = Entity::from_bits(id as u64);
    if world.is_alive(e) { Ok(e) } else { Err(format!("entity {id} does not exist")) }
}

/// World height of the first terrain under (x, z).
pub fn terrain_height(world: &World, x: f32, z: f32) -> Option<f32> {
    let terrains = world.storage::<kit_gamesys::Terrain>()?;
    for (e, t) in terrains.iter() {
        let Some(g) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        // Cast straight down through the terrain's local space.
        let inv = g.inverse();
        let top = inv.transform_point3(Vec3::new(x, 1.0e5, z));
        let down = inv.transform_vector3(Vec3::NEG_Y);
        if let Some(d) = t.raycast(top, down, 1.0e6) {
            let hit = g.transform_point3(top + down.normalize() * d);
            return Some(hit.y);
        }
    }
    None
}

fn transform_member(world: &World, e: Entity, member: &str, args: &[Value]) -> R {
    let mut t = world.get_mut::<Transform>(e).ok_or("entity has no transform")?;
    match member {
        "Position" => {
            no_args(args, member)?;
            Ok(vec_value(t.translation))
        }
        "SetPosition" => {
            t.translation = vec3_args(args)?;
            Ok(Value::Null)
        }
        "Move" => {
            t.translation += vec3_args(args)?;
            Ok(Value::Null)
        }
        "Rotation" => {
            no_args(args, member)?;
            Ok(vec_value(t.euler_degrees()))
        }
        "SetRotation" => {
            t.set_euler_degrees(vec3_args(args)?);
            Ok(Value::Null)
        }
        "Rotate" => {
            let d = vec3_args(args)?;
            let delta = Quat::from_euler(glam::EulerRot::XYZ, d.x.to_radians(), d.y.to_radians(), d.z.to_radians());
            t.rotation = (t.rotation * delta).normalize();
            Ok(Value::Null)
        }
        "Scale" => {
            no_args(args, member)?;
            Ok(vec_value(t.scale))
        }
        "SetScale" => {
            t.scale = match args {
                [s] if s.as_f64().is_some() => Vec3::splat(f(s, "scale")?),
                _ => vec3_args(args)?,
            };
            Ok(Value::Null)
        }
        "Forward" => Ok(vec_value(t.forward())),
        "Right" => Ok(vec_value(t.right())),
        "Up" => Ok(vec_value(t.up())),
        "LookAt" => {
            let target = vec3_args(args)?;
            t.look_at(target, Vec3::Y);
            Ok(Value::Null)
        }
        "WorldPosition" => {
            drop(t);
            no_args(args, member)?;
            Ok(vec_value(world.get::<GlobalTransform>(e).map(|g| g.translation()).unwrap_or_default()))
        }
        _ => Err(format!("unknown member `{member}`")),
    }
}

impl EntityHost<'_> {
    /// `Module.Member(args)` acting on object `e` (the script's own object, or
    /// another one through a feature from `GetFeature`).
    fn module_on(&mut self, e: Entity, module: &str, member: &str, args: &[Value]) -> R {
        if !self.world.is_alive(e) {
            return Err("that object no longer exists".into());
        }
        if let Some(r) = self.extra_member(e, module, member, args) {
            return r;
        }
        let w: &World = self.world;
        match module {
            "Transform" => transform_member(w, e, member, args),
            "Input" => {
                let input = w.try_resource::<Input>().ok_or("input is not available")?;
                let key = || {
                    args.first().and_then(|a| a.as_str()).ok_or_else(|| format!("{member}(\"Key\") expects a key name"))
                };
                match member {
                    "Down" => Ok(Value::Bool(input.down(key()?))),
                    "Pressed" => Ok(Value::Bool(input.pressed(key()?))),
                    "Released" => Ok(Value::Bool(input.released(key()?))),
                    "Axis" => match args {
                        [Value::Str(a), Value::Str(b)] => Ok(Value::Float(input.axis(a, b) as f64)),
                        _ => Err("Axis(\"Negative\", \"Positive\") expects two key names".into()),
                    },
                    "Mouse" => Ok(Value::vector(input.mouse.x as f64, input.mouse.y as f64, 0.0)),
                    "MouseDelta" => Ok(Value::vector(input.mouse_delta.x as f64, input.mouse_delta.y as f64, 0.0)),
                    "Scroll" => Ok(Value::Float(input.scroll as f64)),
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Time" => {
                let t = w.resource::<Time>();
                match member {
                    "Delta" => Ok(Value::Float(t.delta as f64)),
                    "Elapsed" => Ok(Value::Float(t.elapsed)),
                    "Frame" => Ok(Value::Int(t.frame as i64)),
                    "Scale" => Ok(Value::Float(t.scale as f64)),
                    "SetScale" => {
                        let scale = args.first().and_then(Value::as_f64).ok_or("SetScale(scale) expects a number")?;
                        drop(t);
                        w.resource_mut::<Time>().scale = scale as f32;
                        Ok(Value::Null)
                    }
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Entity" => match member {
                "Id" => Ok(entity_id(e)),
                "Name" => Ok(Value::Str(name_of(w, e).into())),
                "SetName" => {
                    let n = args.first().map(|a| a.to_string()).ok_or("SetName(\"Name\")")?;
                    if let Some(mut name) = w.get_mut::<Name>(e) {
                        name.0 = n;
                    }
                    Ok(Value::Null)
                }
                "Destroy" => {
                    self.commands.push(Command::Despawn(e));
                    Ok(Value::Null)
                }
                "Parent" => Ok(w.get::<kit_scene::Parent>(e).map(|p| entity_id(p.0)).unwrap_or(Value::Null)),
                "Children" => Ok(Value::list(kit_scene::children(w, e).into_iter().map(entity_id).collect())),
                _ => Err(format!("unknown member `{member}`")),
            },
            "Scene" => match member {
                "Find" => {
                    let n = args.first().and_then(|a| a.as_str()).ok_or("Find(\"Name\")")?;
                    Ok(kit_scene::find_by_name(w, n).map(entity_id).unwrap_or(Value::Null))
                }
                "Exists" => Ok(Value::Bool(args.first().is_some_and(|a| arg_entity(w, a).is_ok()))),
                // Scene.Load("scenes/level2"), Scene.LoadAdditive(path), Scene.Unload(path): applied at the end of the frame.
                "Load" | "LoadAdditive" | "Unload" => {
                    use kit_gamesys::scenes::SceneOp;
                    let path = args.first().and_then(|a| a.as_str()).ok_or_else(|| format!("Scene.{member}(\"scenes/name\")"))?.to_owned();
                    self.commands.push(Command::Scene(match member {
                        "Load" => SceneOp::Load(path),
                        "LoadAdditive" => SceneOp::LoadAdditive(path),
                        _ => SceneOp::Unload(path),
                    }));
                    Ok(Value::Null)
                }
                "Current" => Ok(Value::Str(
                    w.try_resource::<kit_gamesys::scenes::LoadedScenes>().and_then(|l| l.current().map(|p| p.trim_end_matches(".kscene").to_owned())).unwrap_or_default().into(),
                )),
                "Destroy" => {
                    let target = arg_entity(w, args.first().ok_or("Destroy(id)")?)?;
                    self.commands.push(Command::Despawn(target));
                    Ok(Value::Null)
                }
                "Name" => {
                    let target = arg_entity(w, args.first().ok_or("Name(id)")?)?;
                    Ok(Value::Str(w.get::<Name>(target).map(|n| n.0.clone()).unwrap_or_default().into()))
                }
                // Scene.Position(id), Scene.SetPosition(id, v), Scene.Move(id, v), ...
                _ => {
                    let (first, rest) = args.split_first().ok_or_else(|| format!("Scene.{member}(id, ...)"))?;
                    let target = arg_entity(w, first)?;
                    transform_member(w, target, member, rest)
                }
            },
            "Sprite" => {
                let mut s = w.get_mut::<Sprite>(e).ok_or("entity has no sprite")?;
                match member {
                    "Color" => Ok(Value::list(s.color.to_array().iter().map(|c| Value::Float(*c as f64)).collect())),
                    "SetColor" => {
                        s.color = color_args(args)?;
                        Ok(Value::Null)
                    }
                    "SetSize" => {
                        let v = vec3_args(args)?;
                        s.size = glam::Vec2::new(v.x, v.y);
                        Ok(Value::Null)
                    }
                    "SetFlipX" => {
                        s.flip_x = args.first().is_some_and(|a| a.truthy());
                        Ok(Value::Null)
                    }
                    "SetFlipY" => {
                        s.flip_y = args.first().is_some_and(|a| a.truthy());
                        Ok(Value::Null)
                    }
                    "SetVisible" => {
                        s.visible = args.first().is_some_and(|a| a.truthy());
                        Ok(Value::Null)
                    }
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Model" => {
                let mut m = w.get_mut::<Model>(e).ok_or("entity has no model")?;
                match member {
                    "SetColor" => {
                        m.color = color_args(args)?;
                        Ok(Value::Null)
                    }
                    "SetVisible" => {
                        m.visible = args.first().is_some_and(|a| a.truthy());
                        Ok(Value::Null)
                    }
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Physics" => {
                let mut phys = w.try_resource_mut::<Physics>().ok_or("the physics module is not loaded")?;
                let p = &mut phys.0;
                let v3 = |rest: &[Value]| vec3_args(rest);
                match member {
                    "Velocity" => Ok(p.linear_velocity(e).map(vec_value).unwrap_or(Value::Null)),
                    "SetVelocity" => {
                        p.set_linear_velocity(e, v3(args)?);
                        Ok(Value::Null)
                    }
                    "AngularVelocity" => Ok(p.angular_velocity(e).map(vec_value).unwrap_or(Value::Null)),
                    "SetAngularVelocity" => {
                        p.set_angular_velocity(e, v3(args)?);
                        Ok(Value::Null)
                    }
                    "ApplyImpulse" | "Impulse" => {
                        p.apply_impulse(e, v3(args)?);
                        Ok(Value::Null)
                    }
                    "AddForce" => {
                        p.add_force(e, v3(args)?);
                        Ok(Value::Null)
                    }
                    "Gravity" => Ok(vec_value(p.gravity())),
                    "SetGravity" => {
                        p.set_gravity(v3(args)?);
                        Ok(Value::Null)
                    }
                    "Contacts" => Ok(Value::list(p.contacts(e).into_iter().map(entity_id).collect())),
                    "Touching" => {
                        let other = arg_entity(w, args.first().ok_or("Touching(id)")?)?;
                        Ok(Value::Bool(p.contacts(e).contains(&other)))
                    }
                    // Raycast(origin, direction, maxDistance) -> OBJECT { Entity, Distance, Point, Normal } or NULL
                    "Raycast" => {
                        let (origin, dir, max) = match args {
                            [Value::Vector(o), Value::Vector(d)] => (*o, *d, 1000.0),
                            [Value::Vector(o), Value::Vector(d), m] => (*o, *d, f(m, "maxDistance")?),
                            _ => return Err("Raycast(VECTOR origin, VECTOR direction[, FLOAT max])".into()),
                        };
                        let to = |v: [f64; 3]| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
                        Ok(match p.raycast(to(origin), to(dir), max, Some(e)) {
                            Some(hit) => Value::object(vec![
                                ("Entity", entity_id(hit.entity)),
                                ("Distance", Value::Float(hit.distance as f64)),
                                ("Point", vec_value(hit.point)),
                                ("Normal", vec_value(hit.normal)),
                            ]),
                            None => Value::Null,
                        })
                    }
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Terrain" => match member {
                // HeightAt(x, z): world height of the ground at (x, z), or NULL off the terrain.
                "HeightAt" => {
                    let (x, z) = match args {
                        [Value::Vector(v)] => (v[0] as f32, v[2] as f32),
                        [x, z] => (f(x, "x")?, f(z, "z")?),
                        _ => return Err("HeightAt(x, z)".into()),
                    };
                    Ok(terrain_height(w, x, z).map(|h| Value::Float(h as f64)).unwrap_or(Value::Null))
                }
                _ => Err(format!("unknown member `{member}`")),
            },
            "Path" => {
                let mut path = w.get_mut::<kit_gamesys::PathFollow>(e).ok_or("entity has no sketched path")?;
                match member {
                    "Play" => {
                        path.playing = true;
                        Ok(Value::Null)
                    }
                    "Pause" | "Stop" => {
                        path.playing = false;
                        Ok(Value::Null)
                    }
                    "Playing" => Ok(Value::Bool(path.playing)),
                    "Speed" => Ok(Value::Float(path.speed as f64)),
                    "SetSpeed" => {
                        path.speed = f(args.first().ok_or("SetSpeed(speed)")?, "speed")?;
                        Ok(Value::Null)
                    }
                    "Points" => Ok(Value::Int(path.points.len() as i64)),
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Audio" => {
                use kit_gamesys::{AudioCommand, AudioCommands, AudioSource, AudioState};
                let push = |cmd| {
                    w.try_resource_mut::<AudioCommands>().map(|mut c| c.0.push(cmd)).ok_or("audio is not available")
                };
                match member {
                    "PlaySound" => {
                        let clip = args
                            .first()
                            .and_then(|a| a.as_str())
                            .ok_or("PlaySound(\"sounds/file.wav\", volume, pitch)")?;
                        let volume = args.get(1).map(|v| f(v, "volume")).transpose()?.unwrap_or(1.0);
                        let pitch = args.get(2).map(|v| f(v, "pitch")).transpose()?.unwrap_or(1.0);
                        // From this object, with its source's proximity when it has a 3D one.
                        let at = w.get::<AudioSource>(e).filter(|s| s.spatial).map(|s| {
                            let pos = w.get::<GlobalTransform>(e).map(|g| g.translation()).unwrap_or_default();
                            (pos, s.min_distance, s.max_distance)
                        });
                        push(AudioCommand::OneShot { clip: clip.to_owned(), volume, pitch, at })?;
                        Ok(Value::Null)
                    }
                    "StopAll" => {
                        push(AudioCommand::StopAll)?;
                        Ok(Value::Null)
                    }
                    "Play" => {
                        w.get::<AudioSource>(e).ok_or("this object has no Audio Source")?;
                        push(AudioCommand::Play(e))?;
                        Ok(Value::Null)
                    }
                    "Stop" => {
                        push(AudioCommand::Stop(e))?;
                        Ok(Value::Null)
                    }
                    "Playing" => {
                        Ok(Value::Bool(w.try_resource::<AudioState>().is_some_and(|s| s.playing.contains(&e))))
                    }
                    _ => {
                        let mut a = w.get_mut::<AudioSource>(e).ok_or("this object has no Audio Source")?;
                        match member {
                            "Volume" => Ok(Value::Float(a.volume as f64)),
                            "SetVolume" => {
                                a.volume = f(args.first().ok_or("SetVolume(volume)")?, "volume")?.max(0.0);
                                Ok(Value::Null)
                            }
                            "Pitch" => Ok(Value::Float(a.pitch as f64)),
                            "SetPitch" => {
                                a.pitch = f(args.first().ok_or("SetPitch(pitch)")?, "pitch")?.max(0.01);
                                Ok(Value::Null)
                            }
                            "SetLoop" => {
                                a.looping = matches!(args.first(), Some(Value::Bool(true)));
                                Ok(Value::Null)
                            }
                            "Clip" => Ok(Value::Str(a.clip.clone().into())),
                            "SetClip" => {
                                a.clip = args
                                    .first()
                                    .and_then(|v| v.as_str())
                                    .ok_or("SetClip(\"sounds/file.wav\")")?
                                    .to_owned();
                                Ok(Value::Null)
                            }
                            _ => Err(format!("unknown member `{member}`")),
                        }
                    }
                }
            }
            "Log" => {
                let text = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ");
                let src = self.source();
                let mut c = w.resource_mut::<Console>();
                match member {
                    "Info" => c.info(src, text),
                    "Warn" => c.warn(src, text),
                    "Error" => c.error(src, text),
                    _ => return Err(format!("unknown member `{member}`")),
                }
                Ok(Value::Null)
            }
            _ => Err(format!("unknown module `{module}`")),
        }
    }

}

impl Host for EntityHost<'_> {
    fn has_module(&self, name: &str) -> bool {
        MODULES.contains(&name)
    }

    fn call_module(&mut self, module: &str, member: &str, args: &[Value]) -> R {
        self.module_on(self.entity, module, member, args)
    }

    fn feature(&mut self, op: &str, args: &[Value]) -> R {
        self.feature_op(op, args)
    }

    fn call_feature(&mut self, f: &FeatureRef, member: &str, args: &[Value]) -> R {
        let e = feature_target(self.world, f)?;
        if is_engine_feature(&f.name) {
            return self.module_on(e, &f.name, member, args);
        }
        self.call_script(e, &f.name, member, args)
    }

    fn get_feature(&mut self, f: &FeatureRef, field: &str) -> R {
        let e = feature_target(self.world, f)?;
        if field == "Enabled" {
            return Ok(Value::Bool(feature_enabled(self.world, e, &f.name)));
        }
        if is_engine_feature(&f.name) {
            return self.module_on(e, &f.name, field, &[]);
        }
        let states = self.states.as_deref().ok_or("scripts aren't reachable here")?;
        let st = script_for(states, e, &f.name).ok_or_else(|| busy_or_missing(self.world, e, &f.name, e == self.entity))?;
        match &st.object {
            Some(Value::Instance(i)) => i.get(field).ok_or_else(|| {
                if i.class.method(field).is_some() {
                    format!("`{field}` is a method; call it with {field}(...)")
                } else {
                    format!("`{}` has no field `{field}`", i.class.name())
                }
            }),
            _ => Err("this script has no class to read fields from".into()),
        }
    }

    fn set_feature(&mut self, f: &FeatureRef, field: &str, value: Value) -> Result<(), String> {
        let e = feature_target(self.world, f)?;
        if field == "Enabled" {
            return set_feature_enabled(self.world, self.commands, e, &f.name, value.truthy());
        }
        if is_engine_feature(&f.name) {
            return self.module_on(e, &f.name, &format!("Set{field}"), &[value]).map(|_| ());
        }
        let is_self = e == self.entity;
        let states = self.states.as_deref_mut().ok_or("scripts aren't reachable here")?;
        let st = script_for_mut(states, e, &f.name).ok_or_else(|| busy_or_missing(self.world, e, &f.name, is_self))?;
        let obj = st.object.clone().ok_or("this script has no class to set fields on")?;
        st.interp.set_field(&obj, field, value)
    }

    fn display(&mut self, text: &str) {
        let src = self.source();
        self.world.resource_mut::<Console>().info(src, text);
    }

    fn load_source(&mut self, path: &str) -> Option<String> {
        self.res.as_ref()?.read_text(path).ok()
    }

    /// `INVOKE(what[, position[, rotation[, scale]]])` spawns an object now and
    /// gives back its id. `what` is a built-in shape ("Sphere"), a model file
    /// ("models/ship/Ship.kmodel"), or an existing object (name or id) to copy
    /// with its children, components and scripts. INVOKE puts it inside the
    /// object whose script called it (`inside`), so the transform is relative
    /// to that object; INSTANTIATE puts it at the top level, in world space.
    fn invoke(&mut self, args: &[Value], inside: bool) -> R {
        let (what, rest) = args.split_first().ok_or("(\"Sphere\"[, position[, rotation[, scale]]])")?;
        if rest.len() > 3 {
            return Err("expects at most (what, position, rotation, scale)".into());
        }
        let invoker = self.entity;
        let w = &mut *self.world;
        let parent = if inside && w.is_alive(invoker) { Some(invoker) } else { None };
        let e = match what {
            Value::Str(name) => {
                let name: &str = name;
                let shape = kit_gamesys::MeshKind::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(name));
                if let Some(mesh) = shape {
                    spawn_one(w, mesh.label(), Model { mesh, ..Default::default() })?
                } else if name.ends_with(".kfab") {
                    spawn_fab(w, self.res.as_ref(), name)?
                } else if name.ends_with(".kmodel") {
                    let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned());
                    let model = Model { mesh_asset: name.to_owned(), color: Vec4::ONE, ..Default::default() };
                    spawn_one(w, &stem.unwrap_or_else(|| name.to_owned()), model)?
                } else if let Some(original) = kit_scene::find_by_name(w, name) {
                    kit_gamesys::clone_subtree(w, original).map_err(|e| format!("{e:#}"))?
                } else if self.res.as_ref().is_some_and(|r| r.read_text(&format!("fabs/{name}.kfab")).is_ok()) {
                    // A bare name can also mean the Fab of that name.
                    spawn_fab(w, self.res.as_ref(), &format!("fabs/{name}.kfab"))?
                } else {
                    let shapes: Vec<&str> = kit_gamesys::MeshKind::ALL.iter().map(|m| m.label()).collect();
                    return Err(format!(
                        "nothing called \"{name}\": use a shape ({}), a .kmodel or .kfab file, or the name of an object or Fab",
                        shapes.join(", ")
                    ));
                }
            }
            other => {
                let original = arg_entity(w, other).map_err(|_| {
                    format!("expected a shape name, a .kmodel path, or an object (name or id), got {}", other.type_name())
                })?;
                kit_gamesys::clone_subtree(w, original).map_err(|e| format!("{e:#}"))?
            }
        };
        {
            let mut t = w.get_mut::<Transform>(e).ok_or("the new object has no transform")?;
            if let Some(p) = rest.first() {
                t.translation = vec3_args(std::slice::from_ref(p)).map_err(|_| "position must be a VECTOR")?;
            }
            if let Some(r) = rest.get(1) {
                t.set_euler_degrees(vec3_args(std::slice::from_ref(r)).map_err(|_| "rotation must be a VECTOR (degrees)")?);
            }
            if let Some(sc) = rest.get(2) {
                t.scale = match sc.as_f64() {
                    Some(k) => Vec3::splat(k as f32),
                    None => vec3_args(std::slice::from_ref(sc)).map_err(|_| "scale must be a number or a VECTOR")?,
                };
            }
        }
        match parent {
            Some(p) => {
                if !kit_scene::set_parent(w, e, p) {
                    return Err("an object can't be placed inside its own copy".into());
                }
                w.remove::<kit_scene::RootOrder>(e);
            }
            None if w.has::<kit_scene::Parent>(e) => {
                // A copy of a nested object, asked for at the top level.
                kit_scene::remove_parent(w, e);
                let last = kit_scene::roots(w).iter().filter_map(|r| w.get::<kit_scene::RootOrder>(*r).map(|o| o.0)).max();
                w.insert(e, kit_scene::RootOrder(last.map_or(0, |o| o + 1)));
            }
            None => {}
        }
        kit_scene::propagate_transforms(w);
        Ok(entity_id(e))
    }
}

/// Spawn one top-level object with a model, registered like a loaded scene object.
fn spawn_one(world: &mut World, name: &str, model: Model) -> Result<Entity, String> {
    let data = kit_gamesys::EntityData { name: name.to_owned(), model: Some(model), ..Default::default() };
    let spawned = kit_gamesys::load_scene(world, &kit_gamesys::SceneFile { version: 1, entities: vec![data] })
        .map_err(|e| format!("{e:#}"))?;
    spawned.first().copied().ok_or_else(|| "nothing was spawned".to_owned())
}

// ------------------------------------------------------------------ features

fn is_engine_feature(name: &str) -> bool {
    ENGINE_FEATURES.contains(&name)
}

/// `"model"` → `"Model"`; anything else (a script's class name) unchanged.
fn canonical_feature(name: &str) -> String {
    ENGINE_FEATURES.iter().find(|f| f.eq_ignore_ascii_case(name)).map_or_else(|| name.to_owned(), |f| (*f).to_owned())
}

fn feature_target(world: &World, f: &FeatureRef) -> Result<Entity, String> {
    let e = Entity::from_bits(f.target as u64);
    if world.is_alive(e) { Ok(e) } else { Err("that object no longer exists".into()) }
}

/// An object given as an id, a name, or a feature of it.
fn target_of(world: &World, v: &Value) -> Result<Entity, String> {
    match v {
        Value::Str(n) => kit_scene::find_by_name(world, n).ok_or_else(|| format!("no object named \"{n}\"")),
        Value::Feature(f) => feature_target(world, f),
        other => arg_entity(world, other),
    }
}

/// Does `e` carry this engine feature?
fn has_engine_feature(world: &World, e: Entity, name: &str) -> bool {
    use kit_gamesys::*;
    match name {
        "Transform" | "Entity" => true,
        "Model" => world.has::<Model>(e),
        "Sprite" => world.has::<Sprite>(e),
        "Physics" => world.has::<RigidBody>(e),
        "Collider" => world.has::<Collider>(e),
        "Audio" => world.has::<AudioSource>(e),
        "Path" => world.has::<PathFollow>(e),
        "Light" => world.has::<Light>(e),
        "Camera" => world.has::<Camera>(e),
        "Terrain" => world.has::<Terrain>(e),
        "Diagram" => world.has::<Diagram>(e),
        "Particles" => world.has::<ParticleEmitter>(e),
        _ => false,
    }
}

/// Does `state` answer to `name` (its class name, its file name, or "Script")?
fn script_matches(state: &crate::ScriptState, name: &str) -> bool {
    let stem = std::path::Path::new(&state.path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    name.eq_ignore_ascii_case("Script")
        || state.feature_name().eq_ignore_ascii_case(name)
        || stem.eq_ignore_ascii_case(name)
        || state.path.eq_ignore_ascii_case(name)
}

fn script_for<'s>(states: &'s States, e: Entity, name: &str) -> Option<&'s crate::ScriptState> {
    states.get(&e).filter(|s| script_matches(s, name))
}

fn script_for_mut<'s>(states: &'s mut States, e: Entity, name: &str) -> Option<&'s mut crate::ScriptState> {
    states.get_mut(&e).filter(|s| script_matches(s, name))
}

/// Does the object's Script component point at a script answering to `name`?
fn script_component_matches(world: &World, e: Entity, name: &str) -> bool {
    world.get::<kit_gamesys::Script>(e).is_some_and(|s| {
        let stem = std::path::Path::new(&s.path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        !s.path.is_empty() && (name.eq_ignore_ascii_case("Script") || stem.eq_ignore_ascii_case(name) || s.path.eq_ignore_ascii_case(name))
    })
}

fn busy_or_missing(world: &World, e: Entity, name: &str, is_self: bool) -> String {
    if is_self {
        format!("this is the script that's running; use its fields and methods directly instead of through GetFeature")
    } else if script_component_matches(world, e, name) {
        format!("{}'s {name} script is busy (it's the one calling you) or hasn't started yet", name_of(world, e))
    } else {
        format!("{} has no {name} script", name_of(world, e))
    }
}

fn feature_enabled(world: &World, e: Entity, name: &str) -> bool {
    match name {
        "Model" => world.get::<Model>(e).is_some_and(|m| m.visible),
        "Sprite" => world.get::<Sprite>(e).is_some_and(|s| s.visible),
        "Camera" => world.get::<kit_gamesys::Camera>(e).is_some_and(|c| c.active),
        _ => !kit_gamesys::feature_disabled(world, e, if is_engine_feature(name) { name } else { "Script" }),
    }
}

fn set_feature_enabled(world: &mut World, commands: &mut Vec<Command>, e: Entity, name: &str, on: bool) -> Result<(), String> {
    let _ = commands;
    match name {
        "Model" => world.get_mut::<Model>(e).ok_or("no Model")?.visible = on,
        "Sprite" => world.get_mut::<Sprite>(e).ok_or("no Sprite")?.visible = on,
        "Camera" => world.get_mut::<kit_gamesys::Camera>(e).ok_or("no Camera")?.active = on,
        "Transform" | "Entity" => return Err(format!("{name} can't be switched off; use BeActive(FALSE) for the whole object")),
        _ => {
            let key = if is_engine_feature(name) { name.to_owned() } else { "Script".to_owned() };
            if !world.has::<kit_gamesys::DisabledFeatures>(e) {
                world.insert(e, kit_gamesys::DisabledFeatures::default());
            }
            let mut d = world.get_mut::<kit_gamesys::DisabledFeatures>(e).unwrap();
            d.0.retain(|f| !f.eq_ignore_ascii_case(&key));
            if !on {
                d.0.push(key.clone());
            }
            drop(d);
            if key == "Audio" && !on {
                if let Some(mut c) = world.try_resource_mut::<kit_gamesys::AudioCommands>() {
                    c.0.push(kit_gamesys::AudioCommand::Stop(e));
                }
            }
        }
    }
    Ok(())
}

/// Spawn a Fab (`.kfab`, a saved object) and return its root.
fn spawn_fab(world: &mut World, res: Option<&Resources>, path: &str) -> Result<Entity, String> {
    let res = res.ok_or("no project files to load Fabs from")?;
    let text = res.read_text(path).map_err(|e| format!("{path}: {e:#}"))?;
    let scene = kit_gamesys::SceneFile::from_json(&text).map_err(|e| format!("{path}: {e:#}"))?;
    let spawned = kit_gamesys::load_scene(world, &scene).map_err(|e| format!("{path}: {e:#}"))?;
    spawned.first().copied().ok_or_else(|| format!("{path} is empty"))
}

fn feature_value(e: Entity, name: &str) -> Value {
    Value::Feature(std::rc::Rc::new(FeatureRef { target: e.to_bits() as i64, name: name.into() }))
}

impl EntityHost<'_> {
    /// GetFeature / AddFeature / RemoveFeature / HasFeature / BeActive / IsActive.
    fn feature_op(&mut self, op: &str, args: &[Value]) -> R {
        // The first argument is the object unless it names a feature (then it's this object).
        let names_feature = |v: &Value| match v {
            Value::Str(s) => {
                is_engine_feature(&canonical_feature(s)) || s.ends_with(".x") || s.ends_with(".kdiagram") || s.eq_ignore_ascii_case("Script")
            }
            _ => false,
        };
        match op {
            "BeActive" => {
                let (e, on) = match args {
                    [on] => (self.entity, on.truthy()),
                    [t, on] => (target_of(self.world, t)?, on.truthy()),
                    _ => return Err("BeActive(TRUE/FALSE) or BeActive(object, TRUE/FALSE)".into()),
                };
                if on {
                    self.world.remove::<kit_scene::Inactive>(e);
                } else {
                    self.world.insert(e, kit_scene::Inactive);
                    if let Some(mut c) = self.world.try_resource_mut::<kit_gamesys::AudioCommands>() {
                        for d in kit_scene::descendants_and_self(self.world, e) {
                            c.0.push(kit_gamesys::AudioCommand::Stop(d));
                        }
                    }
                }
                return Ok(Value::Null);
            }
            "IsActive" => {
                let e = match args {
                    [] => self.entity,
                    [t] => target_of(self.world, t)?,
                    _ => return Err("IsActive() or IsActive(object)".into()),
                };
                return Ok(Value::Bool(kit_scene::is_active(self.world, e)));
            }
            _ => {}
        }
        let (e, rest) = match args {
            [first, ..] if names_feature(first) || (args.len() == 1 && matches!(first, Value::Str(_))) => (self.entity, args),
            [first, rest @ ..] if !rest.is_empty() => (target_of(self.world, first)?, rest),
            _ => return Err(format!("{op}(\"Feature\") or {op}(object, \"Feature\")")),
        };
        let raw = rest.first().and_then(|v| v.as_str()).ok_or("the feature's name must be a STRING")?.to_owned();
        let name = canonical_feature(&raw);
        let extra = rest.get(1);
        match op {
            "HasFeature" => Ok(Value::Bool(if is_engine_feature(&name) {
                has_engine_feature(self.world, e, &name)
            } else {
                script_component_matches(self.world, e, &name)
                    || self.states.as_deref().is_some_and(|s| script_for(s, e, &name).is_some())
            })),
            "GetFeature" => {
                let found = if is_engine_feature(&name) {
                    has_engine_feature(self.world, e, &name)
                } else {
                    self.states.as_deref().is_some_and(|s| script_for(s, e, &name).is_some())
                        || script_component_matches(self.world, e, &name)
                };
                Ok(if found { feature_value(e, &name) } else { Value::Null })
            }
            "AddFeature" => self.add_feature(e, &raw, &name, extra),
            "RemoveFeature" => {
                use kit_gamesys::*;
                let w = &mut *self.world;
                let removed = match name.as_str() {
                    "Model" => w.remove::<Model>(e).is_some(),
                    "Sprite" => w.remove::<Sprite>(e).is_some(),
                    "Physics" => w.remove::<RigidBody>(e).is_some(),
                    "Collider" => w.remove::<Collider>(e).is_some(),
                    "Audio" => {
                        if let Some(mut c) = w.try_resource_mut::<AudioCommands>() {
                            c.0.push(AudioCommand::Stop(e));
                        }
                        w.remove::<AudioSource>(e).is_some()
                    }
                    "Path" => w.remove::<PathFollow>(e).is_some(),
                    "Light" => w.remove::<Light>(e).is_some(),
                    "Camera" => w.remove::<Camera>(e).is_some(),
                    "Terrain" => w.remove::<Terrain>(e).is_some(),
                    "Diagram" => w.remove::<Diagram>(e).is_some(),
                    "Particles" => w.remove::<ParticleEmitter>(e).is_some(),
                    "Transform" | "Entity" => return Err(format!("every object has a {name}; destroy the object instead")),
                    _ => {
                        if script_component_matches(w, e, &name) || e != self.entity {
                            w.remove::<Script>(e).is_some()
                        } else {
                            false
                        }
                    }
                };
                Ok(Value::Bool(removed))
            }
            _ => Err(format!("unknown feature function {op}")),
        }
    }

    fn add_feature(&mut self, e: Entity, raw: &str, name: &str, extra: Option<&Value>) -> R {
        use kit_gamesys::*;
        let text = extra.and_then(|v| v.as_str()).map(str::to_owned);
        let w = &mut *self.world;
        let shape = || {
            let mesh = text.as_deref().and_then(|t| MeshKind::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(t)));
            mesh.or_else(|| w.get::<Model>(e).map(|m| m.mesh)).unwrap_or(MeshKind::Cube)
        };
        match name {
            "Model" => {
                let mesh = shape();
                let model = match text.as_deref().filter(|t| t.ends_with(".kmodel")) {
                    Some(path) => Model { mesh_asset: path.to_owned(), color: Vec4::ONE, ..Default::default() },
                    None => Model { mesh, ..Default::default() },
                };
                w.insert(e, model);
            }
            "Sprite" => {
                let mut s = Sprite::default();
                if let Some(t) = text {
                    s.texture = t;
                }
                w.insert(e, s);
            }
            "Collider" => {
                let mesh = shape();
                w.insert(e, Collider { shape: ColliderShape::fit(mesh), ..Default::default() });
            }
            "Physics" => {
                // A body needs a shape: fit one to the model when there's none.
                if !w.has::<Collider>(e) {
                    let mesh = shape();
                    w.insert(e, Collider { shape: ColliderShape::fit(mesh), ..Default::default() });
                }
                let kind = match text.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    Some("kinematic") => BodyKind::Kinematic,
                    Some("static") => BodyKind::Static,
                    _ => BodyKind::Dynamic,
                };
                w.insert(e, RigidBody { kind, ..Default::default() });
            }
            "Audio" => {
                let mut a = AudioSource::default();
                if let Some(t) = text {
                    a.clip = t;
                }
                w.insert(e, a);
            }
            "Path" => {
                w.insert(e, PathFollow::default());
            }
            "Particles" => {
                w.insert(e, ParticleEmitter::default());
            }
            "Light" => {
                w.insert(e, Light::default());
            }
            "Camera" => {
                w.insert(e, Camera::default());
            }
            "Diagram" => {
                w.insert(e, Diagram { graph: text.unwrap_or_default() });
            }
            "Terrain" => return Err("add terrain in the editor; scripts can sculpt it with the Terrain module".into()),
            "Transform" | "Entity" => return Ok(feature_value(e, name)),
            _ if raw.ends_with(".kdiagram") => {
                w.insert(e, Diagram { graph: raw.to_owned() });
                return Ok(feature_value(e, "Diagram"));
            }
            _ if raw.ends_with(".x") => {
                if e == self.entity {
                    return Err("an object runs one script; this one already runs this script".into());
                }
                w.insert(e, Script::new(raw));
                // Load it now so the returned feature works right away (Init runs next frame).
                let st = crate::load_state(w, e, raw, &self.res);
                let feature = st.feature_name();
                if let Some(states) = self.states.as_deref_mut() {
                    states.insert(e, st);
                }
                return Ok(feature_value(e, &feature));
            }
            _ => {
                return Err(format!(
                    "unknown feature \"{raw}\": use one of {} or a script (\"scripts/Name.x\")",
                    ENGINE_FEATURES.join(", ")
                ));
            }
        }
        Ok(feature_value(e, name))
    }

    /// `feature.Method(args)` on another object's script, run by its own interpreter.
    fn call_script(&mut self, e: Entity, name: &str, member: &str, args: &[Value]) -> R {
        if e == self.entity {
            return Err(busy_or_missing(self.world, e, name, true));
        }
        let states = self.states.as_deref_mut().ok_or("scripts aren't reachable here")?;
        if script_for(states, e, name).is_none() {
            return Err(busy_or_missing(self.world, e, name, false));
        }
        let mut st = states.remove(&e).unwrap();
        if st.failed {
            states.insert(e, st);
            return Err(format!("{}'s script stopped after an error", name_of(self.world, e)));
        }
        let mut commands = Vec::new();
        let result = {
            let mut host = EntityHost::new(self.world, e, self.res.clone(), &mut commands, Some(&mut *states));
            match st.object.clone() {
                Some(obj) => st.interp.call_method(&obj, member, args.to_vec(), &mut host),
                None => st.interp.call_function(member, args.to_vec(), &mut host),
            }
        };
        crate::take_timers(&mut st, &mut commands);
        self.commands.extend(commands);
        let states = self.states.as_deref_mut().unwrap();
        if self.world.is_alive(e) {
            states.insert(e, st);
        }
        result.map_err(|err| err.message)
    }
}

// ------------------------------------------------------------- new members

/// A layer list for physics queries: one layer number, an INDEX of them, or all.
fn layer_mask(v: Option<&Value>) -> Result<u32, String> {
    match v {
        None | Some(Value::Null) => Ok(u32::MAX),
        Some(Value::List(l)) => {
            let mut mask = 0u32;
            for x in l.borrow().iter() {
                mask |= 1 << x.as_i64().ok_or("layers are numbers 0-31")?.clamp(0, 31);
            }
            Ok(mask)
        }
        Some(x) => Ok(1 << x.as_i64().ok_or("layers are numbers 0-31")?.clamp(0, 31)),
    }
}

fn hit_object(hit: kit_gamesys::RayHit) -> Value {
    Value::object(vec![
        ("Entity", entity_id(hit.entity)),
        ("Distance", Value::Float(hit.distance as f64)),
        ("Point", vec_value(hit.point)),
        ("Normal", vec_value(hit.normal)),
    ])
}

fn vec_arg(v: Option<&Value>, what: &str) -> Result<Vec3, String> {
    match v {
        Some(Value::Vector(a)) => Ok(Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32)),
        _ => Err(format!("{what} must be a VECTOR")),
    }
}

fn num_arg(v: Option<&Value>, what: &str) -> Result<f32, String> {
    v.and_then(Value::as_f64).map(|x| x as f32).ok_or_else(|| format!("{what} must be a number"))
}

/// The object's world matrix as of now (its own transform may have changed
/// earlier this frame, before the engine recomputed world positions).
fn world_matrix(world: &World, e: Entity) -> glam::Mat4 {
    match world.get::<Transform>(e) {
        Some(t) => parent_matrix(world, e) * t.matrix(),
        None => world.get::<GlobalTransform>(e).map(|g| g.0).unwrap_or(glam::Mat4::IDENTITY),
    }
}

fn parent_matrix(world: &World, e: Entity) -> glam::Mat4 {
    world.get::<kit_scene::Parent>(e).map(|p| world_matrix(world, p.0)).unwrap_or(glam::Mat4::IDENTITY)
}

/// Euler degrees in Kit's X-Y-Z order.
fn euler_of(q: Quat) -> Vec3 {
    let (x, y, z) = q.to_euler(glam::EulerRot::XYZ);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

fn quat_of(deg: Vec3) -> Quat {
    Quat::from_euler(glam::EulerRot::XYZ, deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians())
}

/// A rotation whose forward (-Z) looks along `dir`, keeping `up` up.
fn look_rotation(dir: Vec3, up: Vec3) -> Option<Quat> {
    let f = dir.normalize_or_zero();
    if f == Vec3::ZERO {
        return None;
    }
    let mut right = f.cross(up);
    if right.length_squared() < 1e-8 {
        right = f.cross(if f.y.abs() < 0.9 { Vec3::Y } else { Vec3::Z });
    }
    let right = right.normalize();
    let up = right.cross(f);
    Some(Quat::from_mat3(&glam::Mat3::from_cols(right, up, -f)).normalize())
}

/// Where a camera sits and what it sees, for screen <-> world conversions.
struct CameraView {
    pos: Vec3,
    rot: Quat,
    projection: kit_gamesys::Projection,
    screen: glam::Vec2,
}

impl CameraView {
    fn of(world: &World, cam: Entity) -> Result<Self, String> {
        let c = world.get::<kit_gamesys::Camera>(cam).ok_or("that object has no Camera")?;
        let (_, rot, pos) = world_matrix(world, cam).to_scale_rotation_translation();
        let screen = world.try_resource::<Input>().map(|i| i.screen).unwrap_or_default();
        let screen = if screen.x > 0.0 && screen.y > 0.0 { screen } else { glam::Vec2::new(1280.0, 720.0) };
        Ok(Self { pos, rot, projection: c.projection, screen })
    }

    fn aspect(&self) -> f32 {
        self.screen.x / self.screen.y.max(1.0)
    }

    /// A ray from the camera through a pixel: (origin, direction).
    fn ray(&self, px: glam::Vec2) -> (Vec3, Vec3) {
        let x = px.x / self.screen.x * 2.0 - 1.0;
        let y = 1.0 - px.y / self.screen.y * 2.0;
        match self.projection {
            kit_gamesys::Projection::Perspective { fov_y, .. } => {
                let t = (fov_y.to_radians() * 0.5).tan();
                let dir = self.rot * Vec3::new(x * t * self.aspect(), y * t, -1.0);
                (self.pos, dir.normalize())
            }
            kit_gamesys::Projection::Orthographic { height, .. } => {
                let half = height * 0.5;
                let origin = self.pos + self.rot * Vec3::new(x * half * self.aspect(), y * half, 0.0);
                (origin, self.rot * Vec3::NEG_Z)
            }
        }
    }

    /// Pixel position of a world point (Z is the distance in front; negative = behind).
    fn to_screen(&self, p: Vec3) -> Vec3 {
        let local = self.rot.inverse() * (p - self.pos);
        let depth = -local.z;
        let (x, y) = match self.projection {
            kit_gamesys::Projection::Perspective { fov_y, .. } => {
                let t = (fov_y.to_radians() * 0.5).tan();
                let d = depth.abs().max(1e-6);
                (local.x / (d * t * self.aspect()), local.y / (d * t))
            }
            kit_gamesys::Projection::Orthographic { height, .. } => {
                let half = height * 0.5;
                (local.x / (half * self.aspect()), local.y / half)
            }
        };
        Vec3::new((x + 1.0) * 0.5 * self.screen.x, (1.0 - y) * 0.5 * self.screen.y, depth)
    }
}

/// What the pointer at `px` (game-view pixels) is over: a ray from the active
/// camera, cast into the physics world.
pub fn pointer_hit(world: &World, px: glam::Vec2) -> Option<kit_gamesys::RayHit> {
    let cam = kit_gamesys::active_camera(world)?;
    let view = CameraView::of(world, cam).ok()?;
    let (origin, dir) = view.ray(px);
    world.try_resource::<kit_gamesys::Physics>()?.0.raycast(origin, dir, 10_000.0, None)
}

fn color_value(c: Vec4) -> Value {
    Value::list(c.to_array().iter().map(|x| Value::Float(*x as f64)).collect())
}

impl EntityHost<'_> {
    /// Members added on top of each module's originals. `None` = not handled here.
    fn extra_member(&mut self, e: Entity, module: &str, member: &str, args: &[Value]) -> Option<R> {
        let r = match (module, member) {
            // ---- Time: timers
            ("Time", "After" | "Every") => (|| {
                let seconds = num_arg(args.first(), "the delay in seconds")? as f64;
                let method = args.get(1).and_then(|v| v.as_str()).ok_or("the method name must be a STRING")?.to_owned();
                if e != self.entity {
                    return Err("timers call this script's own methods".into());
                }
                let now = self.world.resource::<kit_core::Time>().elapsed;
                let id = NEXT_TIMER.fetch_add(1, Ordering::Relaxed);
                let every = (member == "Every").then_some(seconds.max(1e-3));
                self.commands.push(Command::AddTimer(Timer {
                    id,
                    due: now + seconds.max(0.0),
                    every,
                    method,
                    args: args[2..].to_vec(),
                }));
                Ok(Value::Int(id as i64))
            })(),
            ("Time", "Start") => (|| {
                let method = args.first().and_then(|v| v.as_str()).ok_or("Start(\"Method\", ...)")?.to_owned();
                if e != self.entity {
                    return Err("coroutines run this script's own methods".into());
                }
                let id = NEXT_TIMER.fetch_add(1, Ordering::Relaxed);
                self.commands.push(Command::StartCoroutine { id, method, args: args[1..].to_vec() });
                Ok(Value::Int(id as i64))
            })(),
            ("Time", "Stop") => match args.first().and_then(Value::as_i64) {
                Some(id) => {
                    self.commands.push(Command::StopCoroutine(id as u64));
                    Ok(Value::Null)
                }
                None => Err("Stop(coroutine) takes what Start gave back".into()),
            },
            ("Time", "Cancel") => match args.first().and_then(Value::as_i64) {
                Some(id) => {
                    self.commands.push(Command::CancelTimer(id as u64));
                    Ok(Value::Null)
                }
                None => Err("Cancel(timer) takes what After or Every gave back".into()),
            },

            // ---- Diagram: talk to graphs
            ("Diagram", "Emit") => (|| {
                let name = args.first().and_then(|v| v.as_str()).ok_or("Emit(\"EventName\"[, value])")?.to_owned();
                let payload = match args.get(1) {
                    Some(v) => Some(to_bridge(v).ok_or("this value can't be sent to a diagram")?),
                    None => None,
                };
                let mut q = self.world.try_resource_mut::<kit_gamesys::GraphEvents>().ok_or("diagrams are off")?;
                q.0.push(kit_gamesys::GraphEvent { name, payload, sender: Some(e) });
                Ok(Value::Null)
            })(),

            // ---- Transform: world / local conversions and turning
            ("Transform", "ToWorld") => vec_arg(args.first(), "the point").map(|p| vec_value(world_matrix(self.world, e).transform_point3(p))),
            ("Transform", "ToLocal") => vec_arg(args.first(), "the point")
                .map(|p| vec_value(world_matrix(self.world, e).inverse().transform_point3(p))),
            ("Transform", "ToWorldDirection") => vec_arg(args.first(), "the direction")
                .map(|d| vec_value(world_matrix(self.world, e).transform_vector3(d))),
            ("Transform", "ToLocalDirection") => vec_arg(args.first(), "the direction")
                .map(|d| vec_value(world_matrix(self.world, e).inverse().transform_vector3(d))),
            ("Transform", "SetWorldPosition") => vec3_args(args).map(|p| {
                let local = parent_matrix(self.world, e).inverse().transform_point3(p);
                if let Some(mut t) = self.world.get_mut::<Transform>(e) {
                    t.translation = local;
                }
                kit_scene::propagate_transforms(self.world);
                Value::Null
            }),
            ("Transform", "WorldRotation") => {
                let (_, r, _) = world_matrix(self.world, e).to_scale_rotation_translation();
                Ok(vec_value(euler_of(r)))
            }
            ("Transform", "SetWorldRotation") => vec3_args(args).map(|deg| {
                let (_, parent, _) = parent_matrix(self.world, e).to_scale_rotation_translation();
                if let Some(mut t) = self.world.get_mut::<Transform>(e) {
                    t.rotation = (parent.inverse() * quat_of(deg)).normalize();
                }
                kit_scene::propagate_transforms(self.world);
                Value::Null
            }),
            ("Transform", "LookAt") if args.len() == 2 => (|| {
                let target = vec_arg(args.first(), "the target")?;
                let up = vec_arg(args.get(1), "up")?;
                let mut t = self.world.get_mut::<Transform>(e).ok_or("no transform")?;
                if let Some(q) = look_rotation(target - t.translation, up) {
                    t.rotation = q;
                }
                Ok(Value::Null)
            })(),
            ("Transform", "TurnToward") => (|| {
                let target = vec_arg(args.first(), "the target")?;
                let max = num_arg(args.get(1), "the most degrees to turn")?;
                let mut t = self.world.get_mut::<Transform>(e).ok_or("no transform")?;
                if let Some(goal) = look_rotation(target - t.translation, Vec3::Y) {
                    let angle = t.rotation.angle_between(goal).to_degrees();
                    let k = if angle <= max.max(0.0) { 1.0 } else { max.max(0.0) / angle };
                    t.rotation = t.rotation.slerp(goal, k).normalize();
                }
                Ok(Value::Null)
            })(),

            // ---- Entity: tags
            ("Entity", "Tags") => Ok(Value::list(
                self.world.get::<kit_gamesys::Tags>(e).map(|t| t.0.iter().map(|s| Value::Str(s.as_str().into())).collect()).unwrap_or_default(),
            )),
            ("Entity", "HasTag") => match args.first().and_then(|v| v.as_str()) {
                Some(tag) => Ok(Value::Bool(self.world.get::<kit_gamesys::Tags>(e).is_some_and(|t| t.0.iter().any(|x| x.eq_ignore_ascii_case(tag))))),
                None => Err("HasTag(\"Tag\")".into()),
            },
            ("Entity", "AddTag" | "RemoveTag") => match args.first().and_then(|v| v.as_str()) {
                Some(tag) => {
                    if !self.world.has::<kit_gamesys::Tags>(e) {
                        self.world.insert(e, kit_gamesys::Tags::default());
                    }
                    let mut t = self.world.get_mut::<kit_gamesys::Tags>(e).unwrap();
                    t.0.retain(|x| !x.eq_ignore_ascii_case(tag));
                    if member == "AddTag" {
                        t.0.push(tag.to_owned());
                    }
                    Ok(Value::Null)
                }
                None => Err(format!("{member}(\"Tag\")")),
            },

            // ---- Scene: finding groups of objects
            ("Scene", "FindAll") => match args.first().and_then(|v| v.as_str()) {
                Some(n) => Ok(Value::list(
                    self.world.storage::<Name>().map(|s| s.iter().filter(|(_, x)| x.0 == n).map(|(e, _)| entity_id(e)).collect()).unwrap_or_default(),
                )),
                None => Err("FindAll(\"Name\")".into()),
            },
            ("Scene", "FindTagged") => match args.first().and_then(|v| v.as_str()) {
                Some(tag) => Ok(Value::list(
                    self.world
                        .storage::<kit_gamesys::Tags>()
                        .map(|s| s.iter().filter(|(_, t)| t.0.iter().any(|x| x.eq_ignore_ascii_case(tag))).map(|(e, _)| entity_id(e)).collect())
                        .unwrap_or_default(),
                )),
                None => Err("FindTagged(\"Tag\")".into()),
            },

            // ---- Model: color, visibility, animation
            ("Model", _) if matches!(
                member,
                "Color" | "Visible" | "Play" | "Animation" | "AnimationTime" | "SetAnimationTime" | "AnimationSpeed"
                    | "SetAnimationSpeed" | "Pause" | "Resume" | "Playing" | "SetLoop" | "Loop"
            ) => (|| {
                let mut m = self.world.get_mut::<Model>(e).ok_or("this object has no Model")?;
                Ok(match member {
                    "Color" => color_value(m.color),
                    "Visible" => Value::Bool(m.visible),
                    "Play" => {
                        m.animation = args.first().map(|v| v.as_str().unwrap_or_default().to_owned()).unwrap_or_default();
                        m.animation_enabled = true;
                        m.clock = Some(0.0);
                        Value::Null
                    }
                    "Animation" => Value::Str(m.animation.as_str().into()),
                    "AnimationTime" => Value::Float(m.clock.unwrap_or(0.0)),
                    "SetAnimationTime" => {
                        m.clock = Some(num_arg(args.first(), "the time")?.max(0.0) as f64);
                        Value::Null
                    }
                    "AnimationSpeed" => Value::Float(m.animation_speed as f64),
                    "SetAnimationSpeed" => {
                        // Keep the pose: switch to the model's own clock at the current point.
                        if m.clock.is_none() {
                            let now = 0.0;
                            m.clock = Some(now);
                        }
                        m.animation_speed = num_arg(args.first(), "the speed")?.max(0.0);
                        Value::Null
                    }
                    "Pause" => {
                        m.clock.get_or_insert(0.0);
                        m.animation_enabled = false;
                        Value::Null
                    }
                    "Resume" => {
                        m.animation_enabled = true;
                        Value::Null
                    }
                    "Playing" => Value::Bool(m.animation_enabled),
                    "Loop" => Value::Bool(m.animation_loop),
                    _ => {
                        m.animation_loop = args.first().is_some_and(|v| v.truthy());
                        Value::Null
                    }
                })
            })(),

            // ---- Physics: area queries, sweeps, layers, body settings
            ("Physics", "FindNear") => (|| {
                let center = vec_arg(args.first(), "the center")?;
                let radius = num_arg(args.get(1), "the radius")?;
                let layers = layer_mask(args.get(2))?;
                let phys = self.world.try_resource::<kit_gamesys::Physics>().ok_or("the physics module is not loaded")?;
                Ok(Value::list(phys.0.overlap_sphere(center, radius, Some(e), layers).into_iter().map(entity_id).collect()))
            })(),
            ("Physics", "SweepSphere") => (|| {
                let origin = vec_arg(args.first(), "the start")?;
                let dir = vec_arg(args.get(1), "the direction")?;
                let radius = num_arg(args.get(2), "the radius")?;
                let max = args.get(3).and_then(Value::as_f64).unwrap_or(1000.0) as f32;
                let layers = layer_mask(args.get(4))?;
                let phys = self.world.try_resource::<kit_gamesys::Physics>().ok_or("the physics module is not loaded")?;
                Ok(phys.0.sphere_cast(origin, dir, radius, max, Some(e), layers).map(hit_object).unwrap_or(Value::Null))
            })(),
            ("Physics", "Raycast") if args.len() == 4 => (|| {
                let origin = vec_arg(args.first(), "the origin")?;
                let dir = vec_arg(args.get(1), "the direction")?;
                let max = num_arg(args.get(2), "the max distance")?;
                let layers = layer_mask(args.get(3))?;
                let phys = self.world.try_resource::<kit_gamesys::Physics>().ok_or("the physics module is not loaded")?;
                Ok(phys.0.raycast_layers(origin, dir, max, Some(e), layers).map(hit_object).unwrap_or(Value::Null))
            })(),
            ("Physics", "Kind" | "SetKind" | "Mass" | "SetMass" | "GravityScale" | "SetGravityScale") => (|| {
                use kit_gamesys::{BodyKind, RigidBody};
                let mut b = self.world.get_mut::<RigidBody>(e).ok_or("this object has no Physics body")?;
                Ok(match member {
                    "Kind" => Value::Str(b.kind.label().into()),
                    "SetKind" => {
                        let k = args.first().and_then(|v| v.as_str()).unwrap_or_default();
                        b.kind = BodyKind::ALL
                            .into_iter()
                            .find(|x| x.label().eq_ignore_ascii_case(k))
                            .ok_or("Kind is \"Dynamic\", \"Kinematic\" or \"Static\"")?;
                        Value::Null
                    }
                    "Mass" => Value::Float(b.mass as f64),
                    "SetMass" => {
                        b.mass = num_arg(args.first(), "the mass")?.max(1e-3);
                        Value::Null
                    }
                    "GravityScale" => Value::Float(b.gravity_scale as f64),
                    _ => {
                        b.gravity_scale = num_arg(args.first(), "the gravity scale")?;
                        Value::Null
                    }
                })
            })(),

            // ---- Collider
            ("Collider", _) => (|| {
                let mut c = self.world.get_mut::<kit_gamesys::Collider>(e).ok_or("this object has no Collider")?;
                Ok(match member {
                    "Layer" => Value::Int(c.layer as i64),
                    "SetLayer" => {
                        c.layer = args.first().and_then(Value::as_i64).ok_or("SetLayer(0-31)")?.clamp(0, 31) as u8;
                        Value::Null
                    }
                    "Trigger" => Value::Bool(c.trigger),
                    "SetTrigger" => {
                        c.trigger = args.first().is_some_and(|v| v.truthy());
                        Value::Null
                    }
                    "Bounciness" => Value::Float(c.restitution as f64),
                    "SetBounciness" => {
                        c.restitution = num_arg(args.first(), "bounciness")?.clamp(0.0, 1.0);
                        Value::Null
                    }
                    "Friction" => Value::Float(c.friction as f64),
                    "SetFriction" => {
                        c.friction = num_arg(args.first(), "friction")?.max(0.0);
                        Value::Null
                    }
                    _ => return Err(format!("unknown member `{member}`")),
                })
            })(),

            // ---- Light
            ("Light", _) => (|| {
                let mut l = self.world.get_mut::<kit_gamesys::Light>(e).ok_or("this object has no Light")?;
                Ok(match member {
                    "Color" => vec_value(l.color),
                    "SetColor" => {
                        let c = color_args(args)?;
                        l.color = c.truncate();
                        Value::Null
                    }
                    "Brightness" => Value::Float(l.intensity as f64),
                    "SetBrightness" => {
                        l.intensity = num_arg(args.first(), "brightness")?.max(0.0);
                        Value::Null
                    }
                    _ => return Err(format!("unknown member `{member}`")),
                })
            })(),

            // ---- Camera
            ("Camera", _) => self.camera_member(e, member, args),

            // ---- Input: mouse capture, gamepads, touch, actions
            ("Input", _) if matches!(
                member,
                "LockMouse" | "ShowMouse" | "MouseLocked" | "ScreenSize" | "Stick" | "Trigger" | "Bind" | "Unbind" | "Bound"
                    | "AnyPressed" | "Touches" | "TouchCount"
            ) => (|| {
                let mut input = self.world.try_resource_mut::<Input>().ok_or("input is not available")?;
                let side = || args.first().and_then(|v| v.as_str()).unwrap_or("Left").to_ascii_lowercase();
                Ok(match member {
                    "LockMouse" => {
                        input.mouse_locked = args.first().is_none_or(|v| v.truthy());
                        Value::Null
                    }
                    "ShowMouse" => {
                        input.mouse_hidden = !args.first().is_none_or(|v| v.truthy());
                        Value::Null
                    }
                    "MouseLocked" => Value::Bool(input.mouse_locked),
                    "ScreenSize" => Value::vector(input.screen.x as f64, input.screen.y as f64, 0.0),
                    "Stick" => {
                        let s = side();
                        Value::vector(input.analog(&format!("{s}x")) as f64, input.analog(&format!("{s}y")) as f64, 0.0)
                    }
                    "Trigger" => Value::Float(input.analog(&format!("{}trigger", side())) as f64),
                    "Bind" => {
                        let action = args.first().and_then(|v| v.as_str()).ok_or("Bind(\"Action\", \"Key\", ...)")?;
                        let mut keys = Vec::new();
                        for v in &args[1..] {
                            match v {
                                Value::List(l) => keys.extend(l.borrow().iter().map(|k| k.to_string())),
                                other => keys.push(other.to_string()),
                            }
                        }
                        input.bind(action, &keys);
                        Value::Null
                    }
                    "Unbind" => {
                        input.unbind(args.first().and_then(|v| v.as_str()).ok_or("Unbind(\"Action\")")?);
                        Value::Null
                    }
                    "Bound" => Value::list(
                        input.bound(args.first().and_then(|v| v.as_str()).unwrap_or_default()).into_iter().map(|k| Value::Str(k.into())).collect(),
                    ),
                    "AnyPressed" => input.any_pressed().map(|k| Value::Str(k.into())).unwrap_or(Value::Null),
                    "Touches" => Value::list(input.touches.iter().map(|(_, p)| Value::vector(p.x as f64, p.y as f64, 0.0)).collect()),
                    _ => Value::Int(input.touches.len() as i64),
                })
            })(),

            // ---- Interface: change the on-screen interface by element id
            ("Interface", _) => (|| {
                let id = args.first().and_then(|v| v.as_str()).ok_or_else(|| format!("{member}(\"element-id\", ...)"))?.to_owned();
                let mut ui = self.world.try_resource_mut::<kit_gamesys::InterfaceEdits>().ok_or("interfaces are off")?;
                let mut set = |attr: &str, value: String| {
                    let list = ui.attributes.entry(id.clone()).or_default();
                    list.retain(|(a, _)| a != attr);
                    list.push((attr.to_owned(), value));
                };
                match member {
                    "SetText" => set("text", args.get(1).map(|v| v.to_string()).unwrap_or_default()),
                    "SetColor" | "SetBackground" => {
                        let c = color_args(&args[1..])?;
                        let hex = format!(
                            "#{:02x}{:02x}{:02x}{:02x}",
                            (c.x.clamp(0.0, 1.0) * 255.0) as u8,
                            (c.y.clamp(0.0, 1.0) * 255.0) as u8,
                            (c.z.clamp(0.0, 1.0) * 255.0) as u8,
                            (c.w.clamp(0.0, 1.0) * 255.0) as u8
                        );
                        set(if member == "SetColor" { "color" } else { "background" }, hex);
                    }
                    "Show" => {
                        ui.visible.insert(id, true);
                    }
                    "Hide" => {
                        ui.visible.insert(id, false);
                    }
                    _ => return Err(format!("unknown member `{member}`")),
                }
                Ok(Value::Null)
            })(),

            // ---- Cinematic: the Cinematics Circuit
            ("Cinematic", _) => (|| {
                let mut c = self.world.try_resource_mut::<kit_gamesys::CinematicControl>().ok_or("cinematics are off")?;
                let need = |c: &kit_gamesys::CinematicControl| {
                    if c.available { Ok(()) } else { Err("install the Cinematics Circuit (Project → Circuit Manager)".to_owned()) }
                };
                Ok(match member {
                    "Play" => {
                        need(&c)?;
                        let name = args.first().and_then(|v| v.as_str()).ok_or("Play(\"CinematicName\")")?;
                        c.requests.push(kit_gamesys::CinematicRequest::Play(name.to_owned()));
                        // Report it as playing straight away, so a loop waiting on it doesn't slip through.
                        c.playing = Some(name.to_owned());
                        Value::Null
                    }
                    "Stop" => {
                        c.requests.push(kit_gamesys::CinematicRequest::Stop);
                        Value::Null
                    }
                    "Skip" => {
                        c.requests.push(kit_gamesys::CinematicRequest::Skip);
                        Value::Null
                    }
                    // Without the Circuit nothing ever plays, so games still run.
                    "Playing" => Value::Bool(c.playing.is_some()),
                    "Current" => c.playing.clone().map(|n| Value::Str(n.into())).unwrap_or(Value::Null),
                    "Shot" => Value::Int(c.shot as i64),
                    "Time" => Value::Float(c.time as f64),
                    "Available" => Value::Bool(c.available),
                    _ => return Err(format!("unknown member `{member}`")),
                })
            })(),

            // ---- Save: keep values between plays
            ("Save", _) => crate::save::member(self.world, member, args),

            // ---- Debug: draw lines in the viewport, time things
            ("Debug", _) => self.debug_member(member, args),
            _ => return None,
        };
        Some(r)
    }

    fn camera_member(&mut self, e: Entity, member: &str, args: &[Value]) -> R {
        use kit_gamesys::{Camera, Projection};
        match member {
            "MakeActive" => {
                let cams: Vec<Entity> = self.world.query::<(Camera,)>();
                for c in cams {
                    if let Some(mut cam) = self.world.get_mut::<Camera>(c) {
                        cam.active = c == e;
                    }
                }
                return Ok(Value::Null);
            }
            "Active" => return Ok(Value::Bool(kit_gamesys::active_camera(self.world) == Some(e))),
            "ScreenToRay" | "WorldToScreen" => {
                // Through this camera if the object has one, else the one showing the game.
                let cam = if self.world.has::<Camera>(e) { Some(e) } else { kit_gamesys::active_camera(self.world) };
                let view = CameraView::of(self.world, cam.ok_or("there's no active camera")?)?;
                return Ok(if member == "ScreenToRay" {
                    let px = vec_arg(args.first(), "the screen position")?;
                    let (origin, dir) = view.ray(glam::Vec2::new(px.x, px.y));
                    Value::object(vec![("Origin", vec_value(origin)), ("Direction", vec_value(dir))])
                } else {
                    vec_value(view.to_screen(vec_arg(args.first(), "the point")?))
                });
            }
            _ => {}
        }
        let mut c = self.world.get_mut::<Camera>(e).ok_or("this object has no Camera")?;
        match (member, &mut c.projection) {
            ("FieldOfView", Projection::Perspective { fov_y, .. }) => Ok(Value::Float(*fov_y as f64)),
            ("SetFieldOfView", Projection::Perspective { fov_y, .. }) => {
                *fov_y = num_arg(args.first(), "the field of view")?.clamp(1.0, 179.0);
                Ok(Value::Null)
            }
            ("Size", Projection::Orthographic { height, .. }) => Ok(Value::Float(*height as f64)),
            ("SetSize", Projection::Orthographic { height, .. }) => {
                *height = num_arg(args.first(), "the size")?.max(0.01);
                Ok(Value::Null)
            }
            ("FieldOfView" | "SetFieldOfView", _) => Err("this camera is orthographic; use Size / SetSize".into()),
            ("Size" | "SetSize", _) => Err("this camera is perspective; use FieldOfView / SetFieldOfView".into()),
            _ => Err(format!("unknown member `{member}`")),
        }
    }

    fn debug_member(&mut self, member: &str, args: &[Value]) -> R {
        let now = self.world.resource::<kit_core::Time>().elapsed;
        let color = |i: usize| match args.get(i) {
            Some(v @ (Value::Vector(_) | Value::List(_))) => color_args(std::slice::from_ref(v)).unwrap_or(Vec4::new(1.0, 0.9, 0.2, 1.0)),
            _ => Vec4::new(1.0, 0.9, 0.2, 1.0),
        };
        let until = |i: usize| now + args.get(i).and_then(Value::as_f64).unwrap_or(0.0);
        let mut lines = Vec::new();
        match member {
            "Line" => lines.push((vec_arg(args.first(), "from")?, vec_arg(args.get(1), "to")?, color(2), until(3))),
            "Ray" => {
                let o = vec_arg(args.first(), "the origin")?;
                lines.push((o, o + vec_arg(args.get(1), "the direction")?, color(2), until(3)));
            }
            "Sphere" => {
                let c = vec_arg(args.first(), "the center")?;
                let r = num_arg(args.get(1), "the radius")?;
                let (col, end) = (color(2), until(3));
                for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
                    for i in 0..24 {
                        let t0 = i as f32 / 24.0 * std::f32::consts::TAU;
                        let t1 = (i + 1) as f32 / 24.0 * std::f32::consts::TAU;
                        lines.push((c + (a * t0.cos() + b * t0.sin()) * r, c + (a * t1.cos() + b * t1.sin()) * r, col, end));
                    }
                }
            }
            "Clock" => {
                // Seconds from a steady clock, for timing your own code.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                    return Ok(Value::Float(START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()));
                }
                // The web has no such clock here: game time is the closest.
                #[cfg(target_arch = "wasm32")]
                return Ok(Value::Float(now));
            }
            _ => return Err(format!("unknown member `{member}`")),
        }
        let mut d = self.world.try_resource_mut::<kit_gamesys::DebugDraw>().ok_or("debug drawing is off")?;
        d.lines.extend(lines.into_iter().map(|(from, to, color, until)| kit_gamesys::DebugLine { from, to, color, until }));
        Ok(Value::Null)
    }
}

/// An x value as a diagram value (for `Diagram.Emit` and Run Script results).
pub fn to_bridge(v: &Value) -> Option<kit_gamesys::BridgeValue> {
    use kit_gamesys::BridgeValue as B;
    Some(match v {
        Value::Bool(b) => B::Bool(*b),
        Value::Int(i) => B::Int(*i),
        Value::Float(f) => B::Float(*f),
        Value::Str(s) => B::Text(s.to_string()),
        Value::Vector(v) => B::Vector3([v[0] as f32, v[1] as f32, v[2] as f32]),
        Value::Feature(f) => B::Entity(Some(f.target as u64)),
        Value::List(l) if l.borrow().len() == 4 && l.borrow().iter().all(|x| x.as_f64().is_some()) => {
            let l = l.borrow();
            B::Color(std::array::from_fn(|i| l[i].as_f64().unwrap() as f32))
        }
        _ => return None,
    })
}

/// A diagram value as an x value (Run Script arguments).
pub fn from_bridge(v: &kit_gamesys::BridgeValue) -> Value {
    use kit_gamesys::BridgeValue as B;
    match v {
        B::Bool(b) => Value::Bool(*b),
        B::Int(i) => Value::Int(*i),
        B::Float(f) => Value::Float(*f),
        B::Text(s) => Value::Str(s.as_str().into()),
        B::Vector3(v) => Value::vector(v[0] as f64, v[1] as f64, v[2] as f64),
        B::Color(c) => Value::list(c.iter().map(|x| Value::Float(*x as f64)).collect()),
        B::Entity(Some(bits)) => Value::Int(*bits as i64),
        B::Entity(None) => Value::Null,
    }
}
