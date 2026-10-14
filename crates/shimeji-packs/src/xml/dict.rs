//! Tag and attribute names shared by the two dictionaries.
//!
//! The root element picks the dictionary. `Behaviour` and `Behavior` are the
//! same tag. `RegistanceX` and `ResistanceX` are the same attribute.

use shimeji_core::model::Schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaDict {
    Japanese,
    English,
}

impl SchemaDict {
    pub(crate) fn from_root(name: &str) -> Option<Self> {
        match name {
            "マスコット" => Some(Self::Japanese),
            "Mascot" => Some(Self::English),
            _ => None,
        }
    }

    pub(crate) fn schema(self) -> Schema {
        match self {
            Self::Japanese => Schema::Japanese,
            Self::English => Schema::English,
        }
    }

    pub(crate) fn tag(self, name: &str) -> Tag {
        match self {
            Self::Japanese => super::dict_ja::tag(name),
            Self::English => super::dict_en::tag(name),
        }
    }

    pub(crate) fn attr(self, name: &str) -> Option<CanonicalAttr> {
        match self {
            Self::Japanese => super::dict_ja::attr(name),
            Self::English => super::dict_en::attr(name),
        }
    }

    pub(crate) fn action_type(self, name: &str) -> Option<PlainType> {
        match self {
            Self::Japanese => super::dict_ja::action_type(name),
            Self::English => super::dict_en::action_type(name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    Mascot,
    Constant,
    ActionList,
    Action,
    ActionReference,
    BehaviorList,
    Behavior,
    BehaviorReference,
    NextBehaviorList,
    Condition,
    Animation,
    Pose,
    Hotspot,
    Information,
    InfoName,
    PreviewImage,
    SplashImage,
    Artist,
    Scripter,
    Commissioner,
    Support,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlainType {
    Embedded,
    Move,
    Stay,
    Animate,
    Sequence,
    Select,
}

impl PlainType {
    pub(crate) fn is_complex(self) -> bool {
        matches!(self, Self::Sequence | Self::Select)
    }
}

/// Canonical attribute. `key` is what the model stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalAttr {
    Name,
    Type,
    Class,
    BorderType,
    Condition,
    Duration,
    Loop,
    BornX,
    BornY,
    BornBehavior,
    BornMascot,
    BornTransient,
    BornInterval,
    BornCount,
    TargetX,
    TargetY,
    TargetBehavior,
    TargetLook,
    VelocityParam,
    InitialVx,
    InitialVy,
    ResistanceX,
    ResistanceY,
    Gravity,
    IeOffsetX,
    IeOffsetY,
    OffsetX,
    OffsetY,
    OffsetType,
    X,
    Y,
    LookRight,
    TransformBehavior,
    TransformMascot,
    Affordance,
    Draggable,
    Frequency,
    Hidden,
    Toggleable,
    Add,
    Action,
    Image,
    ImageRight,
    ImageAnchor,
    Velocity,
    Sound,
    Volume,
    IsTurn,
    Shape,
    Origin,
    Size,
    HotspotBehavior,
    Value,
    Characteristics,
    Breed,
    Scan,
    /// Contributor link. This is not the display name.
    Url,
}

impl CanonicalAttr {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Type",
            Self::Class => "Class",
            Self::BorderType => "BorderType",
            Self::Condition => "Condition",
            Self::Duration => "Duration",
            Self::Loop => "Loop",
            Self::BornX => "BornX",
            Self::BornY => "BornY",
            Self::BornBehavior => "BornBehavior",
            Self::BornMascot => "BornMascot",
            Self::BornTransient => "BornTransient",
            Self::BornInterval => "BornInterval",
            Self::BornCount => "BornCount",
            Self::TargetX => "TargetX",
            Self::TargetY => "TargetY",
            Self::TargetBehavior => "TargetBehavior",
            Self::TargetLook => "TargetLook",
            Self::VelocityParam => "VelocityParam",
            Self::InitialVx => "InitialVX",
            Self::InitialVy => "InitialVY",
            Self::ResistanceX => "ResistanceX",
            Self::ResistanceY => "ResistanceY",
            Self::Gravity => "Gravity",
            Self::IeOffsetX => "IeOffsetX",
            Self::IeOffsetY => "IeOffsetY",
            Self::OffsetX => "OffsetX",
            Self::OffsetY => "OffsetY",
            Self::OffsetType => "OffsetType",
            Self::X => "X",
            Self::Y => "Y",
            Self::LookRight => "LookRight",
            Self::TransformBehavior => "TransformBehavior",
            Self::TransformMascot => "TransformMascot",
            Self::Affordance => "Affordance",
            Self::Draggable => "Draggable",
            Self::Frequency => "Frequency",
            Self::Hidden => "Hidden",
            Self::Toggleable => "Toggleable",
            Self::Add => "Add",
            Self::Action => "Action",
            Self::Image => "Image",
            Self::ImageRight => "ImageRight",
            Self::ImageAnchor => "ImageAnchor",
            Self::Velocity => "Velocity",
            Self::Sound => "Sound",
            Self::Volume => "Volume",
            Self::IsTurn => "IsTurn",
            Self::Shape => "Shape",
            Self::Origin => "Origin",
            Self::Size => "Size",
            Self::HotspotBehavior => "Behavior",
            Self::Value => "Value",
            Self::Characteristics => "Characteristics",
            Self::Breed => "Breed",
            Self::Scan => "Scan",
            Self::Url => "URL",
        }
    }

    /// Dropped from a behavior's parameter map. The reference removes these
    /// before the behavior's extra attributes are copied onto the action.
    pub(crate) fn is_behavior_structural(self) -> bool {
        matches!(
            self,
            Self::Name
                | Self::Action
                | Self::Frequency
                | Self::Hidden
                | Self::Condition
                | Self::Toggleable
        )
    }
}
