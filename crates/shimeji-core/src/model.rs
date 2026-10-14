//! In-memory pack model. Unit 6 reads this file; the loader in `shimeji-packs` fills it.
//!
//! Child actions under an `Embedded` action are kept as data but must not run, because the reference ignores them.
//!
//! The shape follows Shimeji-ee's builders (`Configuration`, `ActionBuilder`,
//! `BehaviorBuilder`, `BehaviorRef`, `AnimationBuilder`), with the choices below
//! kept on purpose.
//!
//! **Schemas.** The actions document's root tag selects Japanese (`マスコット`)
//! or English (`Mascot`). Each action stores that schema because the reference
//! captures it when the action is built. The behaviors document may use the
//! other schema; [`Character::behavior_schema`] is that document, and required
//! behavior names follow it. Engine-written names
//! ([`Schema::engine_variables`]) come from the same `ResourceBundle` lookup:
//! Japanese overrides `FootX` to `footX` and `TargetX`/`TargetY` to `目的地X`/
//! `目的地Y`, and inherits `FootDX`, `VelocityX`, and `VelocityY` from the
//! English parent bundle.
//!
//! **Parameters.** Every XML attribute is kept. `key` is the canonical English
//! name (`BornX`, `ResistanceX`, `TargetBehavior`). `source_name` is the
//! attribute as written (`生まれる場所X`, `RegistanceX`, `BornBehaviour`).
//! Both the `RegistanceX`/`RegistanceY` misspelling and the corrected spelling
//! map to `ResistanceX`/`ResistanceY`. Both `Behaviour` and `Behavior` spellings
//! map to the same key. A name that is not in the schema stays as its own key,
//! which is how the reference leaves user attributes in the variable map.
//!
//! **Scripts.** A value whose whole text is `${…}` or `#{…}` is parsed with
//! `shimeji-expr` at load time. The raw text, including the wrapper, is always
//! kept. `${}` is once per action `init`; `#{}` is cleared on that action's
//! `next`. Constants (`<定数>` / `<Constant>`) are the exception: they stay raw
//! strings, matching `VariableMap.put` of the constant table (`maxCount` "50"
//! is a string, and numeric comparison is coercion at eval time).
//!
//! **Maps.** [`OrderedMap`] is a `LinkedHashMap`: a redefined name keeps its
//! first position and takes the last definition. The reference throws on a
//! duplicate action or behavior; this model follows the plan and keeps the
//! last one instead.
//!
//! **Behaviors.** Entries are flat. Each one carries the `条件` / `Condition`
//! chain from its ancestor groups, outer group first, then its own attribute.
//! [`Character::behavior_tree`] is that nesting for the UI only. Several
//! `次の行動リスト` / `NextBehaviorList` elements append their references, and
//! the last list's `追加` / `Add` wins (`next_additive`; `true` when there is
//! no list, as in `BehaviorBuilder`). A `行動参照` / `BehaviorReference` stores
//! its own attributes and frequency. It does not copy the named behavior's
//! parameters. The reference builds the action from the named behavior, then
//! overlays the reference's parameters, and the next choice still uses the
//! named behavior's next list.
//!
//! **Times and speeds** on poses are ticks and pixels per tick. Nothing here
//! converts them to seconds.
//!
//! **Classes.** The ten classic built-ins resolve from the Java class name and
//! from the short name. Recognised Shimeji-ee classes are stored as
//! [`ActionKind::FastFollow`] so a later unit can run them. An unknown class
//! becomes [`ActionKind::Stay`] with `degraded_from` set; the pack still loads.
//!
//! **Fast-follow fields** (affordance, hotspots, `ImageRight`, breed extensions,
//! `IsTurn`, sound, `Hidden` is not fast-follow, `Toggleable`, `info`) are
//! ordinary data. `Hidden` is a real flag on [`Behavior`].

use std::collections::HashMap;

use shimeji_expr::{Expr, ParseError};

/// Which tag dictionary produced a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    Japanese,
    English,
}

/// Names the engine writes into the action variable map, per schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineVariables {
    pub foot_x: &'static str,
    pub foot_dx: &'static str,
    pub velocity_x: &'static str,
    pub velocity_y: &'static str,
    pub target_x: &'static str,
    pub target_y: &'static str,
}

impl Schema {
    pub fn engine_variables(self) -> EngineVariables {
        match self {
            Self::Japanese => EngineVariables {
                foot_x: "footX",
                foot_dx: "FootDX",
                velocity_x: "VelocityX",
                velocity_y: "VelocityY",
                target_x: "目的地X",
                target_y: "目的地Y",
            },
            Self::English => EngineVariables {
                foot_x: "FootX",
                foot_dx: "FootDX",
                velocity_x: "VelocityX",
                velocity_y: "VelocityY",
                target_x: "TargetX",
                target_y: "TargetY",
            },
        }
    }

    /// Behavior names the reference requires, in the spellings of this schema.
    pub fn required_behaviors(self) -> &'static [&'static str] {
        match self {
            Self::Japanese => &[
                "マウスの周りに集まる",
                "落下する",
                "ドラッグされる",
                "投げられる",
            ],
            Self::English => &["ChaseMouse", "Fall", "Dragged", "Thrown"],
        }
    }
}

/// `枠` / `BorderType` value. Both schemas' words are accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderKind {
    Floor,
    Wall,
    Ceiling,
}

pub fn parse_border(value: &str) -> Option<BorderKind> {
    match value {
        "Floor" | "地面" => Some(BorderKind::Floor),
        "Wall" | "壁" => Some(BorderKind::Wall),
        "Ceiling" | "天井" => Some(BorderKind::Ceiling),
        _ => None,
    }
}

/// How a directory of files was recognised. See `shimeji-packs` layout docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    /// `img/<Set>/` character folders.
    ImageSets,
    /// `conf/` plus images directly in `img/`. One character.
    ClassicSingle,
    /// Action and behavior files at the pack root, images in `img/`.
    RootXml,
    /// PNGs at the pack root and config in `conf/`.
    DroppedImageSet,
}

/// Map that keeps the first insertion position and the last value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedMap<V> {
    order: Vec<String>,
    values: HashMap<String, V>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> OrderedMap<V> {
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            values: HashMap::new(),
        }
    }

    /// Insert `value`. A key that is already present stays where it is.
    pub fn insert(&mut self, key: impl Into<String>, value: V) -> Option<V> {
        let key = key.into();
        if !self.values.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.values.insert(key, value)
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.values.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.order
            .iter()
            .filter_map(|key| self.values.get(key).map(|value| (key.as_str(), value)))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    /// Consume the map in insertion order.
    pub fn into_pairs(self) -> Vec<(String, V)> {
        let mut values = self.values;
        self.order
            .into_iter()
            .filter_map(|key| values.remove(&key).map(|value| (key, value)))
            .collect()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    /// Visit each value in insertion order. The closure must not re-enter the map.
    pub fn for_each_value_mut(&mut self, mut visit: impl FnMut(&mut V)) {
        for key in &self.order {
            if let Some(value) = self.values.get_mut(key) {
                visit(value);
            }
        }
    }
}

/// `${}` once per action init, `#{}` every tick of that action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    Once,
    EveryTick,
}

/// An attribute value. `raw` is the XML text. `ast` is set when a wrapper parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptSource {
    pub raw: String,
    /// Set when the whole value is wrapped, even if the interior failed to parse.
    pub wrapper: Option<ScriptKind>,
    pub ast: Option<Expr>,
}

impl ScriptSource {
    pub fn raw(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            wrapper: None,
            ast: None,
        }
    }

    /// Parse a `${…}` / `#{…}` wrapper. Other text stays raw, with no error.
    ///
    /// The source is always returned. A syntax error is the `Some` beside it:
    /// the wrapper is still recorded and `ast` stays empty.
    pub fn parse_attribute(raw: &str) -> (Self, Option<ParseError>) {
        let Some((kind, inner)) = split_wrapper(raw) else {
            return (Self::raw(raw), None);
        };
        match shimeji_expr::parse(inner) {
            Ok(ast) => (
                Self {
                    raw: raw.to_string(),
                    wrapper: Some(kind),
                    ast: Some(ast),
                },
                None,
            ),
            Err(error) => (
                Self {
                    raw: raw.to_string(),
                    wrapper: Some(kind),
                    ast: None,
                },
                Some(error),
            ),
        }
    }
}

fn split_wrapper(raw: &str) -> Option<(ScriptKind, &str)> {
    let kind = if raw.starts_with("${") {
        ScriptKind::Once
    } else if raw.starts_with("#{") {
        ScriptKind::EveryTick
    } else {
        return None;
    };
    if raw.ends_with('}') && raw.len() >= 3 {
        Some((kind, &raw[2..raw.len() - 1]))
    } else {
        None
    }
}

/// One XML attribute, canonical key plus the name that was written.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub key: String,
    pub source_name: String,
    pub value: ScriptSource,
    pub line: u32,
}

/// `<定数>` / `<Constant>`. `value` is not parsed and not coerced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constant {
    pub value: String,
    pub line: u32,
}

/// The ten built-ins the classic XML uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Fall,
    Jump,
    Dragged,
    Regist,
    Breed,
    Look,
    Offset,
    WalkWithIe,
    FallWithIe,
    ThrowIe,
}

impl Builtin {
    pub fn java_name(self) -> &'static str {
        match self {
            Self::Fall => "Fall",
            Self::Jump => "Jump",
            Self::Dragged => "Dragged",
            Self::Regist => "Regist",
            Self::Breed => "Breed",
            Self::Look => "Look",
            Self::Offset => "Offset",
            Self::WalkWithIe => "WalkWithIE",
            Self::FallWithIe => "FallWithIE",
            Self::ThrowIe => "ThrowIE",
        }
    }
}

/// Shimeji-ee classes stored as data. The engine decides which of these run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastFollow {
    BreedJump,
    BreedMove,
    Broadcast,
    BroadcastJump,
    BroadcastMove,
    BroadcastStay,
    ComplexJump,
    ComplexMove,
    Interact,
    ScanInteract,
    ScanJump,
    ScanMove,
    Transform,
    SelfDestruct,
    Mute,
    Turn,
    MoveWithTurn,
}

impl FastFollow {
    pub fn java_name(self) -> &'static str {
        match self {
            Self::BreedJump => "BreedJump",
            Self::BreedMove => "BreedMove",
            Self::Broadcast => "Broadcast",
            Self::BroadcastJump => "BroadcastJump",
            Self::BroadcastMove => "BroadcastMove",
            Self::BroadcastStay => "BroadcastStay",
            Self::ComplexJump => "ComplexJump",
            Self::ComplexMove => "ComplexMove",
            Self::Interact => "Interact",
            Self::ScanInteract => "ScanInteract",
            Self::ScanJump => "ScanJump",
            Self::ScanMove => "ScanMove",
            Self::Transform => "Transform",
            Self::SelfDestruct => "SelfDestruct",
            Self::Mute => "Mute",
            Self::Turn => "Turn",
            Self::MoveWithTurn => "MoveWithTurn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassResolution {
    Builtin(Builtin),
    FastFollow(FastFollow),
    Unknown,
}

const ACTION_PACKAGE: &str = "com.group_finity.mascot.action.";

/// Resolve `Class` / `クラス`. A dotted name counts only under the Java package.
pub fn resolve_class(name: &str) -> ClassResolution {
    let short = if let Some(rest) = name.strip_prefix(ACTION_PACKAGE) {
        if rest.is_empty() || rest.contains('.') {
            return ClassResolution::Unknown;
        }
        rest
    } else if name.contains('.') {
        return ClassResolution::Unknown;
    } else {
        name
    };
    if let Some(builtin) = builtin_named(short) {
        ClassResolution::Builtin(builtin)
    } else if let Some(follow) = fast_follow_named(short) {
        ClassResolution::FastFollow(follow)
    } else {
        ClassResolution::Unknown
    }
}

fn builtin_named(name: &str) -> Option<Builtin> {
    Some(match name {
        "Fall" => Builtin::Fall,
        "Jump" => Builtin::Jump,
        "Dragged" => Builtin::Dragged,
        "Regist" => Builtin::Regist,
        "Breed" => Builtin::Breed,
        "Look" => Builtin::Look,
        "Offset" => Builtin::Offset,
        "WalkWithIE" => Builtin::WalkWithIe,
        "FallWithIE" => Builtin::FallWithIe,
        "ThrowIE" => Builtin::ThrowIe,
        _ => return None,
    })
}

fn fast_follow_named(name: &str) -> Option<FastFollow> {
    Some(match name {
        "BreedJump" => FastFollow::BreedJump,
        "BreedMove" => FastFollow::BreedMove,
        "Broadcast" => FastFollow::Broadcast,
        "BroadcastJump" => FastFollow::BroadcastJump,
        "BroadcastMove" => FastFollow::BroadcastMove,
        "BroadcastStay" => FastFollow::BroadcastStay,
        "ComplexJump" => FastFollow::ComplexJump,
        "ComplexMove" => FastFollow::ComplexMove,
        "Interact" => FastFollow::Interact,
        "ScanInteract" => FastFollow::ScanInteract,
        "ScanJump" => FastFollow::ScanJump,
        "ScanMove" => FastFollow::ScanMove,
        "Transform" => FastFollow::Transform,
        "SelfDestruct" => FastFollow::SelfDestruct,
        "Mute" => FastFollow::Mute,
        "Turn" => FastFollow::Turn,
        "MoveWithTurn" => FastFollow::MoveWithTurn,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Move,
    Stay,
    Animate,
    Sequence,
    Select,
    Builtin(Builtin),
    FastFollow(FastFollow),
}

/// A pose frame. Velocities are pixels per tick. `duration_ticks` is ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pose {
    pub image_source: Option<String>,
    pub image: Option<ResolvedImage>,
    pub image_right_source: Option<String>,
    pub image_right: Option<ResolvedImage>,
    /// `name-r.png` next to the left image, when that file exists.
    pub right_sibling: Option<ResolvedImage>,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub velocity_x: i32,
    pub velocity_y: i32,
    pub duration_ticks: i32,
    pub sound: Option<String>,
    pub volume: Option<String>,
    pub line: u32,
}

/// An image path that resolved inside the image-set root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImage {
    /// Attribute text, such as `/shime1.png`.
    pub source: String,
    /// NFC path relative to the image-set root. This is the on-disk name,
    /// which can differ in case from `source`.
    pub relative: String,
    /// IHDR pixel size. Zero when the header cannot be read. A missing file is `None`.
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub condition: Option<ScriptSource>,
    pub is_turn: bool,
    pub poses: Vec<Pose>,
    pub hotspots: Vec<Hotspot>,
    pub line: u32,
}

/// Click region. Parsed and kept; the engine does not use it in this unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotspot {
    pub shape: String,
    pub origin: String,
    pub size: String,
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: i32,
    pub height: i32,
    pub behavior: Option<String>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionRef {
    pub name: String,
    /// Every attribute on the reference, including `Name`. The reference's
    /// values win over the caller's when the action is built.
    pub params: OrderedMap<Param>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionNode {
    Action(Action),
    Reference(ActionRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    /// `None` for an action nested inside Sequence or Select.
    pub name: Option<String>,
    pub schema: Schema,
    pub kind: ActionKind,
    /// The `Class` attribute when `kind` was rewritten to Stay.
    pub degraded_from: Option<String>,
    /// Every attribute, in document order. Known keys use the canonical name.
    pub params: OrderedMap<Param>,
    pub animations: Vec<Animation>,
    pub children: Vec<ActionNode>,
    pub line: u32,
}

impl Action {
    pub fn visit<'a>(&'a self, visit: &mut dyn FnMut(&'a Action)) {
        visit(self);
        for child in &self.children {
            if let ActionNode::Action(action) = child {
                action.visit(visit);
            }
        }
    }
}

/// A reference inside a next-list. Parameters are its own attributes only.
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRef {
    pub name: String,
    pub frequency: i32,
    /// Conditions from ancestor groups inside this next-list, then its own.
    /// The enclosing behavior's conditions are not included.
    pub conditions: Vec<ScriptSource>,
    pub params: OrderedMap<Param>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NextList {
    pub add: bool,
    pub nodes: Vec<BehaviorNode>,
    pub refs: Vec<BehaviorRef>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Behavior {
    pub name: String,
    /// `動作` / `Action` attribute, or the behavior name when it was omitted.
    pub action: String,
    pub frequency: i32,
    pub hidden: bool,
    pub toggleable: bool,
    /// Ancestor group conditions, then this behavior's own condition.
    pub conditions: Vec<ScriptSource>,
    /// Attributes other than name, action, frequency, hidden, condition, toggleable.
    pub params: OrderedMap<Param>,
    pub next_lists: Vec<NextList>,
    /// Last next-list's `追加`, or `true` when the behavior has no next-list.
    pub next_additive: bool,
    pub line: u32,
}

impl Behavior {
    pub fn next_refs(&self) -> impl Iterator<Item = &BehaviorRef> {
        self.next_lists.iter().flat_map(|list| list.refs.iter())
    }
}

/// Nesting kept for the UI. Flat entries live on [`Character::behaviors`].
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorNode {
    Group {
        condition: Option<ScriptSource>,
        children: Vec<BehaviorNode>,
        line: u32,
    },
    Behavior {
        name: String,
        line: u32,
    },
    Reference {
        name: String,
        line: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributorRole {
    Artist,
    Scripter,
    Commissioner,
    Support,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contributor {
    pub role: ContributorRole,
    pub name: String,
    pub url: Option<String>,
}

/// `info.xml`, when the pack has one. Absent fields stay empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Information {
    pub display_name: Option<String>,
    pub preview_image: Option<String>,
    pub splash_image: Option<String>,
    pub contributors: Vec<Contributor>,
}

impl Information {
    pub fn set_contributor(&mut self, contributor: Contributor) {
        if let Some(existing) = self
            .contributors
            .iter_mut()
            .find(|item| item.role == contributor.role)
        {
            *existing = contributor;
        } else {
            self.contributors.push(contributor);
        }
    }
}

/// One image set, or the single character of a classic pack.
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    pub name: String,
    /// Schema of the actions document. This selects [`Schema::engine_variables`]
    /// for actions loaded from that file. Each [`Action::schema`] is the schema
    /// of the file that defined that action.
    pub schema: Schema,
    /// Schema of the behaviors document. The reference replaces its schema on
    /// every file and checks required behavior names after the behaviors file,
    /// so those names follow this schema.
    pub behavior_schema: Schema,
    /// Image-set root relative to the pack, NFC, `/`-separated. Empty when the
    /// pack root is the image-set root.
    pub image_root: String,
    pub actions_file: String,
    pub behaviors_file: String,
    pub info_file: Option<String>,
    pub constants: OrderedMap<Constant>,
    pub actions: OrderedMap<Action>,
    pub behaviors: OrderedMap<Behavior>,
    pub behavior_tree: Vec<BehaviorNode>,
    pub info: Information,
}

impl Character {
    pub fn visit_actions<'a>(&'a self, visit: &mut dyn FnMut(&'a Action)) {
        for action in self.actions.values() {
            action.visit(visit);
        }
    }
}

/// A pack loaded from a trusted, already-extracted directory.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub name: String,
    pub layout: LayoutKind,
    pub characters: Vec<Character>,
}

impl Pack {
    pub fn character(&self, name: &str) -> Option<&Character> {
        self.characters
            .iter()
            .find(|character| character.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redefined_key_keeps_its_place_and_takes_the_last_value() {
        let mut map = OrderedMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("a", 3);
        let pairs: Vec<_> = map.iter().map(|(key, value)| (key, *value)).collect();
        assert_eq!(pairs, vec![("a", 3), ("b", 2)]);
    }

    #[test]
    fn class_names_resolve_by_package_and_by_short_name() {
        assert_eq!(
            resolve_class("com.group_finity.mascot.action.Fall"),
            ClassResolution::Builtin(Builtin::Fall)
        );
        assert_eq!(
            resolve_class("ScanMove"),
            ClassResolution::FastFollow(FastFollow::ScanMove)
        );
        assert_eq!(resolve_class("com.example.Fall"), ClassResolution::Unknown);
    }

    #[test]
    fn japanese_engine_names_match_the_bundle_fallback() {
        let names = Schema::Japanese.engine_variables();
        assert_eq!(names.foot_x, "footX");
        assert_eq!(names.foot_dx, "FootDX");
        assert_eq!(names.target_x, "目的地X");
        assert_eq!(Schema::English.engine_variables().foot_x, "FootX");
    }
}
