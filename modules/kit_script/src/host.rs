//! Engine modules exposed to x scripts.

use glam::{Quat, Vec3, Vec4};
use kit_core::{Console, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{Model, Physics, Sprite};
use kit_input::Input;
use kit_resource::Resources;
use kit_scene::{GlobalTransform, Name, Transform};
use kit_x::{Host, Value};

/// Modules scripts can `TAKE`.
pub const MODULES: &[&str] =
    &["Transform", "Input", "Time", "Entity", "Scene", "Sprite", "Model", "Log", "Physics", "Path", "Terrain", "Audio"];

/// Structural changes requested by scripts, applied after the script returns.
pub enum Command {
    Despawn(Entity),
}

pub struct EntityHost<'a> {
    world: &'a World,
    entity: Entity,
    res: Option<Resources>,
    commands: &'a mut Vec<Command>,
}

impl<'a> EntityHost<'a> {
    pub fn new(world: &'a World, entity: Entity, res: Option<Resources>, commands: &'a mut Vec<Command>) -> Self {
        Self { world, entity, res, commands }
    }

    fn source(&self) -> String {
        self.world.get::<Name>(self.entity).map(|n| n.0.clone()).unwrap_or_else(|| format!("{:?}", self.entity))
    }
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

impl Host for EntityHost<'_> {
    fn has_module(&self, name: &str) -> bool {
        MODULES.contains(&name)
    }

    fn call_module(&mut self, module: &str, member: &str, args: &[Value]) -> R {
        let w = self.world;
        let e = self.entity;
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
                    _ => Err(format!("unknown member `{member}`")),
                }
            }
            "Entity" => match member {
                "Id" => Ok(entity_id(e)),
                "Name" => Ok(Value::Str(self.source().into())),
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
                        let clip = args.first().and_then(|a| a.as_str()).ok_or("PlaySound(\"sounds/file.wav\", volume, pitch)")?;
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
                    "Playing" => Ok(Value::Bool(w.try_resource::<AudioState>().is_some_and(|s| s.playing.contains(&e)))),
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
                                a.clip = args.first().and_then(|v| v.as_str()).ok_or("SetClip(\"sounds/file.wav\")")?.to_owned();
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

    fn display(&mut self, text: &str) {
        let src = self.source();
        self.world.resource_mut::<Console>().info(src, text);
    }

    fn load_source(&mut self, path: &str) -> Option<String> {
        self.res.as_ref()?.read_text(path).ok()
    }
}
