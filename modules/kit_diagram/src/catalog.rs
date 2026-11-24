//! The node catalog: every node kind, its typed pins, and its place in the
//! node picker. Only nodes with working runtime behavior are listed.

use kit_gamedata::{GameData, Id, Value, ValueType};
use serde::{Deserialize, Serialize};

/// Local (relative to the parent) or world space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Space {
    #[default]
    World,
    Local,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    #[default]
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl CompareOp {
    pub const ALL: [CompareOp; 6] = [
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessOrEqual,
        CompareOp::Greater,
        CompareOp::GreaterOrEqual,
    ];
    pub fn symbol(self) -> &'static str {
        match self {
            CompareOp::Equal => "=",
            CompareOp::NotEqual => "≠",
            CompareOp::Less => "<",
            CompareOp::LessOrEqual => "≤",
            CompareOp::Greater => ">",
            CompareOp::GreaterOrEqual => "≥",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MathOp {
    #[default]
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl MathOp {
    pub const ALL: [MathOp; 4] = [MathOp::Add, MathOp::Subtract, MathOp::Multiply, MathOp::Divide];
    pub fn symbol(self) -> &'static str {
        match self {
            MathOp::Add => "+",
            MathOp::Subtract => "−",
            MathOp::Multiply => "×",
            MathOp::Divide => "÷",
        }
    }
}

/// Number types Compare and Math work on (Int and Float stay distinct).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumberType {
    Int,
    #[default]
    Float,
    Vector3,
}

impl NumberType {
    pub fn value_type(self) -> ValueType {
        match self {
            NumberType::Int => ValueType::Int,
            NumberType::Float => ValueType::Float,
            NumberType::Vector3 => ValueType::Vector3,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    #[default]
    And,
    Or,
    Not,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Conversion {
    #[default]
    IntToFloat,
    /// Rounds to the nearest whole number.
    FloatToInt,
    FloatToText,
    IntToText,
}

/// Every kind of node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeKind {
    // EVENTS
    OnStart,
    /// The player presses Interact (E) while looking at, or standing near,
    /// this object. See `exec::interact_target`.
    OnInteract,
    OnTriggerEnter,
    OnTriggerExit,
    OnTimer,
    OnCustomEvent {
        event: String,
        payload: Option<ValueType>,
    },

    // OBJECTS
    GetSelf,
    GetPosition {
        space: Space,
    },
    SetPosition {
        space: Space,
    },
    MoveOverTime,
    SetRotation {
        space: Space,
    },
    SetScale,
    SetTransform {
        space: Space,
    },
    SetVisible,

    // APPEARANCE AND PHYSICS
    SetTint,
    SetGravityScale,
    SetSceneGravity,
    ApplyImpulse,
    SetVelocity,
    GetVelocity,

    // GAME DATA, VARIABLES, DECISIONS
    DatasetRow {
        dataset: Id,
        row: Option<Id>,
    },
    Constant {
        constant: Id,
    },
    GetVariable {
        name: String,
    },
    SetVariable {
        name: String,
    },
    Compare {
        op: CompareOp,
        number: NumberType,
    },
    Branch,
    Sequence {
        outputs: u8,
    },
    Delay,
    Math {
        op: MathOp,
        number: NumberType,
    },
    Logic {
        op: BoolOp,
    },
    Convert {
        conversion: Conversion,
    },
    MakeVector3,
    BreakVector3,

    // SCRIPT FIELDS (PUBLIC: read; EDIT: read and write)
    GetScriptField {
        field: String,
        value_type: ValueType,
    },
    SetScriptField {
        field: String,
        value_type: ValueType,
    },

    // INTEGRATION
    PlaySound,
    StopSound,
    EmitEvent {
        event: String,
        payload: Option<ValueType>,
    },
    RunScript {
        function: String,
        args: Vec<ValueType>,
    },
}

/// What flows through a pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinType {
    /// Execution order (white wires).
    Exec,
    /// A typed value (colored wires).
    Data(ValueType),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PinDef {
    /// Stable id (saved in links and inline values).
    pub id: String,
    pub name: String,
    pub ty: PinType,
    /// Default inline value for unconnected inputs.
    pub default: Option<Value>,
}

fn exec(id: &str, name: &str) -> PinDef {
    PinDef { id: id.into(), name: name.into(), ty: PinType::Exec, default: None }
}

fn data(id: &str, name: &str, ty: ValueType) -> PinDef {
    let default = Some(ty.default_value());
    PinDef { id: id.into(), name: name.into(), ty: PinType::Data(ty), default }
}

fn data_default(id: &str, name: &str, value: Value) -> PinDef {
    PinDef { id: id.into(), name: name.into(), ty: PinType::Data(value.value_type()), default: Some(value) }
}

/// Target input for object nodes (unconnected = the node's TargetRef, Self by default).
fn target() -> PinDef {
    PinDef { id: "target".into(), name: "Target".into(), ty: PinType::Data(ValueType::Entity), default: None }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signature {
    pub inputs: Vec<PinDef>,
    pub outputs: Vec<PinDef>,
}

impl Signature {
    pub fn input(&self, id: &str) -> Option<&PinDef> {
        self.inputs.iter().find(|p| p.id == id)
    }
    pub fn output(&self, id: &str) -> Option<&PinDef> {
        self.outputs.iter().find(|p| p.id == id)
    }
}

impl NodeKind {
    /// Section in the node picker.
    pub fn category(&self) -> &'static str {
        use NodeKind::*;
        match self {
            OnStart | OnInteract | OnTriggerEnter | OnTriggerExit | OnTimer | OnCustomEvent { .. } => "EVENTS",
            GetSelf
            | GetPosition { .. }
            | SetPosition { .. }
            | MoveOverTime
            | SetRotation { .. }
            | SetScale
            | SetTransform { .. }
            | SetVisible => "OBJECTS",
            SetTint | SetGravityScale | SetSceneGravity | ApplyImpulse | SetVelocity | GetVelocity => {
                "APPEARANCE & PHYSICS"
            }
            DatasetRow { .. } | Constant { .. } => "DATASETS",
            GetScriptField { .. } | SetScriptField { .. } => "SCRIPT FIELDS",
            GetVariable { .. } | SetVariable { .. } => "VARIABLES",
            Compare { .. }
            | Branch
            | Sequence { .. }
            | Delay
            | Math { .. }
            | Logic { .. }
            | Convert { .. }
            | MakeVector3
            | BreakVector3 => "DECISIONS & MATH",
            PlaySound | StopSound | EmitEvent { .. } | RunScript { .. } => "INTEGRATION",
        }
    }

    pub fn is_event(&self) -> bool {
        self.category() == "EVENTS"
    }

    /// Pure nodes have no exec pins; they're evaluated when a value is needed.
    pub fn is_pure(&self, gd: &GameData) -> bool {
        let sig = self.signature(gd);
        !sig.inputs.iter().chain(&sig.outputs).any(|p| p.ty == PinType::Exec)
    }

    /// Title shown on the node (GameData names resolved by ID).
    pub fn title(&self, gd: &GameData) -> String {
        use NodeKind::*;
        match self {
            OnStart => "On Start".into(),
            OnInteract => "On Interact".into(),
            OnTriggerEnter => "On Trigger Enter".into(),
            OnTriggerExit => "On Trigger Exit".into(),
            OnTimer => "On Timer".into(),
            OnCustomEvent { event, .. } => format!("On Event: {event}"),
            GetSelf => "Self".into(),
            GetPosition { .. } => "Get Position".into(),
            SetPosition { .. } => "Set Position".into(),
            MoveOverTime => "Move Over Time".into(),
            SetRotation { .. } => "Set Rotation".into(),
            SetScale => "Set Scale".into(),
            SetTransform { .. } => "Set Transform".into(),
            SetVisible => "Set Visible".into(),
            SetTint => "Set Tint".into(),
            SetGravityScale => "Set Gravity Scale".into(),
            SetSceneGravity => "Set Scene Gravity".into(),
            ApplyImpulse => "Apply Impulse".into(),
            SetVelocity => "Set Velocity".into(),
            GetVelocity => "Get Velocity".into(),
            DatasetRow { dataset, .. } => match gd.dataset(dataset) {
                Some(ds) => ds.name.clone(),
                None => "Missing dataset".into(),
            },
            Constant { constant } => gd.constant(constant).map_or("Missing constant".into(), |c| c.name.clone()),
            GetVariable { name } => format!("Get {name}"),
            SetVariable { name } => format!("Set {name}"),
            Compare { op, .. } => format!("Compare {}", op.symbol()),
            Branch => "Branch".into(),
            Sequence { .. } => "Sequence".into(),
            Delay => "Delay".into(),
            Math { op, .. } => format!("Math {}", op.symbol()),
            Logic { op } => format!("{op:?}"),
            Convert { conversion } => match conversion {
                Conversion::IntToFloat => "Int → Float".into(),
                Conversion::FloatToInt => "Float → Int".into(),
                Conversion::FloatToText => "Float → Text".into(),
                Conversion::IntToText => "Int → Text".into(),
            },
            MakeVector3 => "Make Vector3".into(),
            BreakVector3 => "Break Vector3".into(),
            GetScriptField { field, .. } => format!("Get {field}"),
            SetScriptField { field, .. } => format!("Set {field}"),
            PlaySound => "Play Sound".into(),
            StopSound => "Stop Sound".into(),
            EmitEvent { event, .. } => format!("Emit Event: {event}"),
            RunScript { function, .. } => format!("Run Script: {function}"),
        }
    }

    /// The node's pins. Dataset rows get one output per field (by field ID).
    pub fn signature(&self, gd: &GameData) -> Signature {
        use NodeKind::*;
        let v3 = |id: &str, name: &str| data(id, name, ValueType::Vector3);
        let ex_in = || exec("exec", "");
        let ex_out = || exec("then", "");
        let (inputs, outputs) = match self {
            OnStart => (vec![], vec![ex_out()]),
            OnTimer => (
                vec![
                    data_default("interval", "Interval (s)", Value::Float(1.0)),
                    data_default("repeat", "Repeat", Value::Bool(true)),
                ],
                vec![ex_out()],
            ),
            OnInteract => (vec![], vec![ex_out(), data("interactor", "Interactor", ValueType::Entity)]),
            OnTriggerEnter | OnTriggerExit => {
                (vec![], vec![ex_out(), data("other", "Other Entity", ValueType::Entity)])
            }
            OnCustomEvent { payload, .. } => {
                let mut outs = vec![ex_out(), data("sender", "Sender", ValueType::Entity)];
                if let Some(t) = payload {
                    outs.push(data("payload", "Payload", t.clone()));
                }
                (vec![], outs)
            }
            GetSelf => (vec![], vec![data("object", "Self", ValueType::Entity)]),
            GetPosition { .. } => (vec![target()], vec![v3("position", "Position")]),
            SetPosition { .. } => (vec![ex_in(), target(), v3("position", "Position")], vec![ex_out()]),
            MoveOverTime => (
                vec![
                    ex_in(),
                    target(),
                    v3("destination", "Destination"),
                    data_default("duration", "Duration (s)", Value::Float(1.0)),
                ],
                vec![ex_out(), exec("completed", "Completed")],
            ),
            SetRotation { .. } => (vec![ex_in(), target(), v3("rotation", "Rotation (°)")], vec![ex_out()]),
            SetScale => {
                (vec![ex_in(), target(), data_default("scale", "Scale", Value::Vector3([1.0; 3]))], vec![ex_out()])
            }
            SetTransform { .. } => {
                (vec![ex_in(), target(), data("transform", "Transform", ValueType::Transform)], vec![ex_out()])
            }
            SetVisible => {
                (vec![ex_in(), target(), data_default("visible", "Visible", Value::Bool(true))], vec![ex_out()])
            }
            SetTint => {
                (vec![ex_in(), target(), data_default("color", "Color", Value::Color([1.0; 4]))], vec![ex_out()])
            }
            SetGravityScale => {
                (vec![ex_in(), target(), data_default("value", "Gravity Scale", Value::Float(1.0))], vec![ex_out()])
            }
            SetSceneGravity => {
                (vec![ex_in(), data_default("gravity", "Gravity", Value::Vector3([0.0, -9.81, 0.0]))], vec![ex_out()])
            }
            ApplyImpulse => (vec![ex_in(), target(), v3("impulse", "Impulse")], vec![ex_out()]),
            SetVelocity => (vec![ex_in(), target(), v3("velocity", "Velocity")], vec![ex_out()]),
            GetVelocity => (vec![target()], vec![v3("velocity", "Velocity")]),
            DatasetRow { dataset, .. } => {
                let ins = vec![data("row", "Row", ValueType::Row(dataset.clone()))];
                let outs = gd
                    .dataset(dataset)
                    .map(|ds| ds.fields.iter().map(|f| data(&f.id, &f.name, f.value_type.clone())).collect())
                    .unwrap_or_default();
                (ins, outs)
            }
            Constant { constant } => {
                let ty = gd.constant(constant).map_or(ValueType::Float, |c| c.value.value_type());
                (vec![], vec![data("value", "Value", ty)])
            }
            GetVariable { .. } => (
                vec![],
                vec![PinDef {
                    id: "value".into(),
                    name: "Value".into(),
                    ty: PinType::Data(ValueType::Float),
                    default: None,
                }],
            ),
            SetVariable { .. } => (
                vec![
                    ex_in(),
                    PinDef {
                        id: "value".into(),
                        name: "Value".into(),
                        ty: PinType::Data(ValueType::Float),
                        default: None,
                    },
                ],
                vec![ex_out()],
            ),
            Compare { number, .. } => {
                let t = number.value_type();
                (vec![data("a", "A", t.clone()), data("b", "B", t)], vec![data("result", "Result", ValueType::Bool)])
            }
            Branch => (
                vec![ex_in(), data("condition", "Condition", ValueType::Bool)],
                vec![exec("true", "True"), exec("false", "False")],
            ),
            Sequence { outputs } => (
                vec![ex_in()],
                (0..(*outputs).max(2)).map(|i| exec(&format!("then{i}"), &format!("Then {i}"))).collect(),
            ),
            Delay => (vec![ex_in(), data_default("seconds", "Seconds", Value::Float(1.0))], vec![ex_out()]),
            Math { number, .. } => {
                let t = number.value_type();
                (vec![data("a", "A", t.clone()), data("b", "B", t.clone())], vec![data("result", "Result", t)])
            }
            Logic { op } => {
                let mut ins = vec![data("a", "A", ValueType::Bool)];
                if *op != BoolOp::Not {
                    ins.push(data("b", "B", ValueType::Bool));
                }
                (ins, vec![data("result", "Result", ValueType::Bool)])
            }
            Convert { conversion } => {
                let (from, to) = match conversion {
                    Conversion::IntToFloat => (ValueType::Int, ValueType::Float),
                    Conversion::FloatToInt => (ValueType::Float, ValueType::Int),
                    Conversion::FloatToText => (ValueType::Float, ValueType::Text),
                    Conversion::IntToText => (ValueType::Int, ValueType::Text),
                };
                (vec![data("value", "Value", from)], vec![data("result", "Result", to)])
            }
            MakeVector3 => (
                vec![
                    data("x", "X", ValueType::Float),
                    data("y", "Y", ValueType::Float),
                    data("z", "Z", ValueType::Float),
                ],
                vec![v3("vector", "Vector")],
            ),
            BreakVector3 => (
                vec![v3("vector", "Vector")],
                vec![
                    data("x", "X", ValueType::Float),
                    data("y", "Y", ValueType::Float),
                    data("z", "Z", ValueType::Float),
                ],
            ),
            GetScriptField { value_type, .. } => (vec![target()], vec![data("value", "Value", value_type.clone())]),
            SetScriptField { value_type, .. } => {
                (vec![ex_in(), target(), data("value", "Value", value_type.clone())], vec![ex_out()])
            }
            PlaySound => (
                vec![
                    ex_in(),
                    target(),
                    data("clip", "Sound (empty = its Audio Source)", ValueType::Text),
                    data_default("volume", "Volume", Value::Float(1.0)),
                ],
                vec![ex_out()],
            ),
            StopSound => (vec![ex_in(), target()], vec![ex_out()]),
            EmitEvent { payload, .. } => {
                let mut ins = vec![ex_in()];
                if let Some(t) = payload {
                    ins.push(data("payload", "Payload", t.clone()));
                }
                (ins, vec![ex_out()])
            }
            RunScript { args, .. } => {
                let mut ins = vec![ex_in(), target()];
                for (i, t) in args.iter().enumerate() {
                    ins.push(data(&format!("arg{i}"), &format!("Arg {i}"), t.clone()));
                }
                (
                    ins,
                    vec![
                        ex_out(),
                        exec("failed", "Failed"),
                        data("result", "Result", ValueType::Text),
                        data("error", "Error", ValueType::Text),
                    ],
                )
            }
        };
        Signature { inputs, outputs }
    }

    /// Variable nodes take their pin type from the graph's declaration.
    pub fn signature_in(&self, gd: &GameData, graph: &crate::graph::Graph) -> Signature {
        let mut sig = self.signature(gd);
        if let NodeKind::GetVariable { name } | NodeKind::SetVariable { name } = self {
            let ty = graph.variable(name).map_or(ValueType::Float, |v| v.value_type());
            for p in sig.inputs.iter_mut().chain(sig.outputs.iter_mut()) {
                if p.id == "value" {
                    p.ty = PinType::Data(ty.clone());
                    p.default = Some(ty.default_value());
                }
            }
        }
        sig
    }

    /// Nodes for a script's PUBLIC and EDIT fields (Get for both, Set for
    /// EDIT only), for the node picker and the Data view.
    pub fn script_field_nodes(fields: &[kit_script::FieldView]) -> Vec<NodeKind> {
        let mut out = Vec::new();
        for f in fields {
            let Some(value_type) = script_value_type(&f.ty) else { continue };
            out.push(NodeKind::GetScriptField { field: f.name.clone(), value_type: value_type.clone() });
            if f.access == kit_script::Access::Edit {
                out.push(NodeKind::SetScriptField { field: f.name.clone(), value_type });
            }
        }
        out
    }

    /// Everything the node picker offers (datasets and constants included,
    /// from the current GameData).
    pub fn catalog(gd: &GameData, graph: &crate::graph::Graph) -> Vec<NodeKind> {
        use NodeKind::*;
        let mut out = vec![
            OnStart,
            OnInteract,
            OnTriggerEnter,
            OnTriggerExit,
            OnTimer,
            OnCustomEvent { event: "MyEvent".into(), payload: None },
            GetSelf,
            GetPosition { space: Space::World },
            SetPosition { space: Space::World },
            MoveOverTime,
            SetRotation { space: Space::World },
            SetScale,
            SetTransform { space: Space::World },
            SetVisible,
            SetTint,
            SetGravityScale,
            SetSceneGravity,
            ApplyImpulse,
            SetVelocity,
            GetVelocity,
        ];
        out.extend(
            gd.datasets.iter().map(|d| DatasetRow { dataset: d.id.clone(), row: d.rows.first().map(|r| r.id.clone()) }),
        );
        out.extend(gd.constants.iter().map(|c| Constant { constant: c.id.clone() }));
        for v in &graph.variables {
            out.push(GetVariable { name: v.name.clone() });
            out.push(SetVariable { name: v.name.clone() });
        }
        out.extend([
            Compare { op: CompareOp::Greater, number: NumberType::Float },
            Branch,
            Sequence { outputs: 2 },
            Delay,
            Math { op: MathOp::Add, number: NumberType::Float },
            Logic { op: BoolOp::And },
            Logic { op: BoolOp::Not },
            Convert { conversion: Conversion::IntToFloat },
            Convert { conversion: Conversion::FloatToInt },
            MakeVector3,
            BreakVector3,
            PlaySound,
            StopSound,
            EmitEvent { event: "MyEvent".into(), payload: None },
            RunScript { function: "MyFunction".into(), args: Vec::new() },
        ]);
        out
    }
}

/// The Diagram type for an x field type (None: not usable in graphs).
pub fn script_value_type(x_type: &str) -> Option<ValueType> {
    Some(match x_type {
        "BOOL" => ValueType::Bool,
        "INTEGER" => ValueType::Int,
        "FLOAT" => ValueType::Float,
        "STRING" => ValueType::Text,
        "VECTOR" => ValueType::Vector3,
        _ => return None,
    })
}
