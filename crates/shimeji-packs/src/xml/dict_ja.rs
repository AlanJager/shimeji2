//! Japanese tags, plus English names the Japanese bundle inherits.
//!
//! `schema_ja.properties` overrides the classic names. Keys it does not define
//! fall through to `schema.properties`, so the XML spelling of those is the
//! English word (`Hidden`, `ImageRight`, `Hotspot`, …).

use super::dict::{CanonicalAttr, PlainType, Tag};

pub(crate) fn tag(name: &str) -> Tag {
    match name {
        "マスコット" => Tag::Mascot,
        "定数" => Tag::Constant,
        "動作リスト" => Tag::ActionList,
        "動作" => Tag::Action,
        "動作参照" => Tag::ActionReference,
        "行動リスト" => Tag::BehaviorList,
        "行動" => Tag::Behavior,
        "行動参照" => Tag::BehaviorReference,
        "次の行動リスト" => Tag::NextBehaviorList,
        "条件" => Tag::Condition,
        "アニメーション" => Tag::Animation,
        "ポーズ" => Tag::Pose,
        "画像" => Tag::Other,
        // Inherited from the English parent bundle.
        "Hotspot" => Tag::Hotspot,
        "Information" => Tag::Information,
        "Name" => Tag::InfoName,
        "PreviewImage" => Tag::PreviewImage,
        "SplashImage" => Tag::SplashImage,
        "Artist" => Tag::Artist,
        "Scripter" => Tag::Scripter,
        "Commissioner" => Tag::Commissioner,
        "Support" => Tag::Support,
        // `Name` is overridden to 名前, including the information element's tag.
        "名前" => Tag::InfoName,
        _ => Tag::Other,
    }
}

pub(crate) fn attr(name: &str) -> Option<CanonicalAttr> {
    Some(match name {
        "名前" => CanonicalAttr::Name,
        "種類" => CanonicalAttr::Type,
        "クラス" => CanonicalAttr::Class,
        "枠" => CanonicalAttr::BorderType,
        "条件" => CanonicalAttr::Condition,
        "長さ" => CanonicalAttr::Duration,
        "繰り返し" => CanonicalAttr::Loop,
        "生まれる場所X" => CanonicalAttr::BornX,
        "生まれる場所Y" => CanonicalAttr::BornY,
        "生まれた時の行動" => CanonicalAttr::BornBehavior,
        "目的地X" => CanonicalAttr::TargetX,
        "目的地Y" => CanonicalAttr::TargetY,
        "速度" => CanonicalAttr::VelocityParam,
        "初速X" => CanonicalAttr::InitialVx,
        "初速Y" => CanonicalAttr::InitialVy,
        "空気抵抗X" => CanonicalAttr::ResistanceX,
        "空気抵抗Y" => CanonicalAttr::ResistanceY,
        "重力" => CanonicalAttr::Gravity,
        "IEの端X" => CanonicalAttr::IeOffsetX,
        "IEの端Y" => CanonicalAttr::IeOffsetY,
        "端X" => CanonicalAttr::OffsetX,
        "端Y" => CanonicalAttr::OffsetY,
        "X" => CanonicalAttr::X,
        "Y" => CanonicalAttr::Y,
        "右向き" => CanonicalAttr::LookRight,
        "動作" => CanonicalAttr::Action,
        "頻度" => CanonicalAttr::Frequency,
        "追加" => CanonicalAttr::Add,
        "画像" => CanonicalAttr::Image,
        "基準座標" => CanonicalAttr::ImageAnchor,
        "移動速度" => CanonicalAttr::Velocity,
        "値" => CanonicalAttr::Value,
        // Parent-bundle fallbacks. The XML uses the English word.
        "Draggable" => CanonicalAttr::Draggable,
        "Affordance" => CanonicalAttr::Affordance,
        "BornMascot" => CanonicalAttr::BornMascot,
        "BornTransient" => CanonicalAttr::BornTransient,
        "BornInterval" => CanonicalAttr::BornInterval,
        "BornCount" => CanonicalAttr::BornCount,
        "Characteristics" => CanonicalAttr::Characteristics,
        "Breed" => CanonicalAttr::Breed,
        "Scan" => CanonicalAttr::Scan,
        "TargetBehavior" | "TargetBehaviour" => CanonicalAttr::TargetBehavior,
        "TargetLook" => CanonicalAttr::TargetLook,
        "OffsetType" => CanonicalAttr::OffsetType,
        "TransformBehavior" | "TransformBehaviour" => CanonicalAttr::TransformBehavior,
        "TransformMascot" => CanonicalAttr::TransformMascot,
        "Hidden" => CanonicalAttr::Hidden,
        "Toggleable" => CanonicalAttr::Toggleable,
        "ImageRight" => CanonicalAttr::ImageRight,
        "Sound" => CanonicalAttr::Sound,
        "Volume" => CanonicalAttr::Volume,
        "IsTurn" => CanonicalAttr::IsTurn,
        "Shape" => CanonicalAttr::Shape,
        "Origin" => CanonicalAttr::Origin,
        "Size" => CanonicalAttr::Size,
        "Behavior" | "Behaviour" => CanonicalAttr::HotspotBehavior,
        "URL" => CanonicalAttr::Url,
        _ => return None,
    })
}

pub(crate) fn action_type(name: &str) -> Option<PlainType> {
    Some(match name {
        "組み込み" => PlainType::Embedded,
        "移動" => PlainType::Move,
        "静止" => PlainType::Stay,
        "固定" => PlainType::Animate,
        "複合" => PlainType::Sequence,
        "選択" => PlainType::Select,
        _ => return None,
    })
}
