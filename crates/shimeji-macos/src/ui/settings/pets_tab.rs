//! Packs, characters, and behaviors. A drop of a folder or a zip imports.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
#[allow(deprecated)]
use objc2_app_kit::NSFilenamesPboardType;
use objc2_app_kit::{
    NSButtonCell, NSButtonType, NSControlTextEditingDelegate, NSDragOperation,
    NSDraggingDestination, NSDraggingInfo, NSOutlineView, NSOutlineViewDataSource,
    NSOutlineViewDelegate, NSPasteboardTypeFileURL, NSScrollView, NSTableColumn, NSTextField,
    NSView,
};
use objc2_foundation::NSInteger;
use objc2_foundation::{NSArray, NSNotification, NSNumber, NSString};

use crate::bridge::ShellClock;

use super::super::{lossy, rect};
use super::form;
use objc2_app_kit::NSAutoresizingMaskOptions as Resize;

const SEP: char = '\u{1f}';

struct Row {
    token: Retained<NSString>,
    label: String,
    children: Vec<usize>,
    check: Option<bool>,
    pack: String,
    character: String,
    behavior: String,
    toggleable: bool,
}

pub struct OutlineIvars {
    clock: Cell<*const ShellClock>,
    rows: RefCell<Vec<Row>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = OutlineIvars]
    pub struct OutlineSource;

    unsafe impl NSObjectProtocol for OutlineSource {}

    unsafe impl NSControlTextEditingDelegate for OutlineSource {}

    unsafe impl NSOutlineViewDataSource for OutlineSource {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        unsafe fn outline_view_number_of_children_of_item(
            &self,
            _outline_view: &NSOutlineView,
            item: Option<&AnyObject>,
        ) -> NSInteger {
            let rows = self.ivars().rows.borrow();
            match item {
                None => rows.iter().filter(|row| is_pack(row)).count() as NSInteger,
                Some(item) => row_for(&rows, item)
                    .map(|row| row.children.len() as NSInteger)
                    .unwrap_or(0),
            }
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        unsafe fn outline_view_child_of_item(
            &self,
            _outline_view: &NSOutlineView,
            index: NSInteger,
            item: Option<&AnyObject>,
        ) -> Retained<AnyObject> {
            let rows = self.ivars().rows.borrow();
            let index = index as usize;
            let child = match item {
                None => rows.iter().filter(|row| is_pack(row)).nth(index),
                Some(item) => row_for(&rows, item)
                    .and_then(|row| row.children.get(index).and_then(|slot| rows.get(*slot))),
            };
            match child {
                Some(row) => unsafe { Retained::cast_unchecked(row.token.clone()) },
                None => unsafe { Retained::cast_unchecked(NSString::from_str("")) },
            }
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        unsafe fn outline_view_is_item_expandable(
            &self,
            _outline_view: &NSOutlineView,
            item: &AnyObject,
        ) -> bool {
            let rows = self.ivars().rows.borrow();
            row_for(&rows, item).is_some_and(|row| !row.children.is_empty())
        }

        #[unsafe(method_id(outlineView:objectValueForTableColumn:byItem:))]
        unsafe fn outline_view_object_value_for_table_column_by_item(
            &self,
            _outline_view: &NSOutlineView,
            table_column: Option<&NSTableColumn>,
            item: Option<&AnyObject>,
        ) -> Option<Retained<AnyObject>> {
            let rows = self.ivars().rows.borrow();
            // `return` and `?` leave the objc shim, whose real return type is not Option.
            match item.and_then(|item| row_for(&rows, item)) {
                None => None,
                Some(row) => {
                    let enabled_column = table_column
                        .map(|column| lossy(&column.identifier()) == "enabled")
                        .unwrap_or(false);
                    if enabled_column {
                        row.check.map(|on| unsafe {
                            Retained::cast_unchecked(NSNumber::numberWithBool(on))
                        })
                    } else {
                        Some(unsafe { Retained::cast_unchecked(NSString::from_str(&row.label)) })
                    }
                }
            }
        }

        #[unsafe(method(outlineView:setObjectValue:forTableColumn:byItem:))]
        unsafe fn outline_view_set_object_value_for_table_column_by_item(
            &self,
            _outline_view: &NSOutlineView,
            object: Option<&AnyObject>,
            table_column: Option<&NSTableColumn>,
            item: Option<&AnyObject>,
        ) {
            let enabled_column = table_column
                .map(|column| lossy(&column.identifier()) == "enabled")
                .unwrap_or(false);
            if !enabled_column {
                return;
            }
            let on = object
                .map(|object| unsafe { msg_send![object, boolValue] })
                .unwrap_or(false);
            let action = {
                let mut rows = self.ivars().rows.borrow_mut();
                let Some(index) = item.and_then(|item| row_index(&rows, item)) else {
                    return;
                };
                rows[index].check = Some(on);
                let row = &rows[index];
                if !row.behavior.is_empty() {
                    if !row.toggleable {
                        return;
                    }
                    Some((
                        row.pack.clone(),
                        row.character.clone(),
                        row.behavior.clone(),
                        true,
                    ))
                } else if !row.character.is_empty() {
                    Some((
                        row.pack.clone(),
                        row.character.clone(),
                        String::new(),
                        false,
                    ))
                } else {
                    None
                }
            };
            let ptr = self.ivars().clock.get();
            if ptr.is_null() {
                return;
            }
            let clock = unsafe { &*ptr };
            if let Some((pack, character, behavior, is_behavior)) = action {
                if is_behavior {
                    super::super::actions::set_behavior_enabled(
                        clock, &pack, &character, &behavior, on,
                    );
                } else {
                    super::super::actions::set_character_enabled(clock, &pack, &character, on);
                }
            }
        }
    }

    unsafe impl NSOutlineViewDelegate for OutlineSource {
        #[unsafe(method(outlineView:willDisplayCell:forTableColumn:item:))]
        unsafe fn outline_view_will_display_cell_for_table_column_item(
            &self,
            _outline: &NSOutlineView,
            cell: &AnyObject,
            column: Option<&NSTableColumn>,
            item: &AnyObject,
        ) {
            if column.is_some_and(|column| lossy(&column.identifier()) == "enabled") {
                let rows = self.ivars().rows.borrow();
                let available = row_for(&rows, item).is_some_and(|row| row.check.is_some());
                unsafe {
                    let _: () = msg_send![cell,setTransparent:!available];
                    let _: () = msg_send![cell,setEnabled:available];
                }
            }
        }

        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn outline_view_selection_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let row: NSInteger = unsafe { msg_send![&*object, selectedRow] };
            if row < 0 {
                return;
            }
            let item: Option<Retained<AnyObject>> = unsafe { msg_send![&*object, itemAtRow: row] };
            let Some(item) = item else {
                return;
            };
            let rows = self.ivars().rows.borrow();
            let Some(found) = row_for(&rows, &item) else {
                return;
            };
            if !found.character.is_empty() || found.pack.is_empty() {
                return;
            }
            let pack_id = found.pack.clone();
            drop(rows);
            let ptr = self.ivars().clock.get();
            if ptr.is_null() {
                return;
            }
            let clock = unsafe { &*ptr };
            let text = {
                let state = clock.ivars().state.borrow();
                let Some(pack) = state.catalog.get(&pack_id) else {
                    return;
                };
                if !pack.degraded() {
                    return;
                }
                pack.diagnostics
                    .iter()
                    .map(|diagnostic| match diagnostic.line {
                        Some(line) => format!("{}:{line}: {}", diagnostic.file, diagnostic.message),
                        None => format!("{}: {}", diagnostic.file, diagnostic.message),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            super::super::actions::set_import_banner(clock, &text);
        }
    }
);

impl OutlineSource {
    fn new(mtm: MainThreadMarker, clock: &ShellClock) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(OutlineIvars {
            clock: Cell::new(std::ptr::from_ref(clock)),
            rows: RefCell::new(Vec::new()),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn rebuild(&self) {
        let ptr = self.ivars().clock.get();
        if ptr.is_null() {
            return;
        }
        let clock = unsafe { &*ptr };
        let state = clock.ivars().state.borrow();
        let mut rows = Vec::new();
        for pack in state.catalog.packs() {
            let pack_index = rows.len();

            let mark = if pack.degraded() { " !" } else { "" };
            rows.push(Row {
                token: NSString::from_str(&format!("pack{SEP}{}", pack.id)),
                label: format!("{} · {}{mark}", pack.id, pack.format_label()),
                children: Vec::new(),
                check: None,
                pack: pack.id.clone(),
                character: String::new(),
                behavior: String::new(),
                toggleable: false,
            });
            for character in &pack.characters {
                let char_index = rows.len();
                rows.push(Row {
                    token: NSString::from_str(&format!(
                        "char{SEP}{}{SEP}{}",
                        pack.id, character.name
                    )),
                    label: character.name.clone(),
                    children: Vec::new(),
                    check: Some(state.settings.character_enabled(&pack.id, &character.name)),
                    pack: pack.id.clone(),
                    character: character.name.clone(),
                    behavior: String::new(),
                    toggleable: false,
                });
                rows[pack_index].children.push(char_index);
                for behavior in &character.behaviors {
                    if behavior.hidden && !behavior.toggleable {
                        continue;
                    }
                    let label = if behavior.hidden {
                        format!("{}（隐藏动作）", behavior.name)
                    } else {
                        behavior.name.clone()
                    };
                    let index = rows.len();
                    rows.push(Row {
                        token: NSString::from_str(&format!(
                            "behavior{SEP}{}{SEP}{}{SEP}{}",
                            pack.id, character.name, behavior.name
                        )),
                        label,
                        children: Vec::new(),
                        check: behavior.toggleable.then(|| {
                            state.settings.behavior_enabled(
                                &pack.id,
                                &character.name,
                                &behavior.name,
                            )
                        }),
                        pack: pack.id.clone(),
                        character: character.name.clone(),
                        behavior: behavior.name.clone(),
                        toggleable: behavior.toggleable,
                    });
                    rows[char_index].children.push(index);
                }
            }
        }
        drop(state);
        *self.ivars().rows.borrow_mut() = rows;
    }
}

pub struct PetsOutlineIvars {
    clock: Cell<*const ShellClock>,
}

define_class!(
    #[unsafe(super(NSOutlineView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PetsOutlineIvars]
    pub struct PetsOutline;

    unsafe impl NSObjectProtocol for PetsOutline {}

    unsafe impl NSDraggingDestination for PetsOutline {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            _sender: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            // `return` would leave the objc shim, which expects `Bool`, not `bool`.
            let ptr = self.ivars().clock.get();
            if ptr.is_null() {
                false
            } else {
                let clock = unsafe { &*ptr };
                let paths = super::super::import_flow::dropped_paths(&sender.draggingPasteboard());
                let root = clock.ivars().state.borrow().catalog.root().to_path_buf();
                let banner = super::super::import_flow::accept_drop(clock, &root, &paths);
                super::super::actions::set_import_banner(clock, &banner);
                !paths.is_empty()
            }
        }
    }
);

impl PetsOutline {
    fn new(mtm: MainThreadMarker, clock: &ShellClock) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PetsOutlineIvars {
            clock: Cell::new(std::ptr::from_ref(clock)),
        });
        let frame = rect(0.0, 0.0, 520.0, 280.0);
        let outline: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        #[allow(deprecated)]
        let filenames = unsafe { NSFilenamesPboardType };
        let file_url = unsafe { NSPasteboardTypeFileURL };
        let types = NSArray::from_slice(&[filenames, file_url]);
        outline.registerForDraggedTypes(&types);
        outline
    }
}

pub struct PetsTab {
    pub view: Retained<NSView>,
    pub outline: Retained<PetsOutline>,
    pub source: Retained<OutlineSource>,
    pub banner: Retained<NSTextField>,
}

pub fn build(mtm: MainThreadMarker, clock: &ShellClock) -> PetsTab {
    let view = form::page(
        mtm,
        "宠物",
        "管理素材包、角色和动作。展开列表可单独启用或停用。",
        660.0,
    );
    let source = OutlineSource::new(mtm, clock);
    let outline = PetsOutline::new(mtm, clock);
    let name =
        NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str("name"));
    name.setTitle(&NSString::from_str("素材包 / 角色 / 动作"));
    name.setWidth(480.0);
    name.setEditable(false);
    outline.addTableColumn(&name);
    unsafe { outline.setOutlineTableColumn(Some(&name)) };
    let enabled = NSTableColumn::initWithIdentifier(
        NSTableColumn::alloc(mtm),
        &NSString::from_str("enabled"),
    );
    enabled.setTitle(&NSString::from_str("启用"));
    enabled.setWidth(56.0);
    enabled.setEditable(true);
    let cell = NSButtonCell::new(mtm);
    cell.setButtonType(NSButtonType::Switch);
    cell.setTitle(Some(&NSString::from_str("")));
    unsafe { enabled.setDataCell(&cell) };
    outline.addTableColumn(&enabled);
    unsafe {
        outline.setDataSource(Some(ProtocolObject::from_ref(&*source)));
        outline.setDelegate(Some(ProtocolObject::from_ref(&*source)));
    }
    let scroll =
        NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(24.0, 178.0, 592.0, 354.0));
    scroll.setHasVerticalScroller(true);
    scroll.setAutoresizingMask(Resize::ViewWidthSizable);
    outline.setRowHeight(26.0);
    outline.setAutoresizingMask(Resize::ViewWidthSizable);
    scroll.setDocumentView(Some(&outline));
    view.addSubview(&scroll);
    let import = form::button(
        &view,
        "导入素材…",
        rect(24.0, 124.0, 132.0, 30.0),
        17,
        unsafe { &*std::ptr::from_ref(clock).cast::<AnyObject>() },
        objc2::sel!(settingsAct:),
        mtm,
    );
    import.setToolTip(Some(&NSString::from_str(
        "选择 Shimeji 文件夹或 ZIP 素材包",
    )));
    form::text(
        &view,
        "也可以将文件夹或 ZIP 直接拖入下方列表。",
        rect(172.0, 129.0, 440.0, 30.0),
        12.0,
        true,
        false,
        mtm,
    );
    let banner = form::text(
        &view,
        "拖入素材包后即可在这里管理。",
        rect(28.0, 552.0, 584.0, 64.0),
        12.0,
        true,
        false,
        mtm,
    );
    PetsTab {
        view,
        outline,
        source,
        banner,
    }
}

fn is_pack(row: &Row) -> bool {
    row.character.is_empty()
}

fn row_index(rows: &[Row], item: &AnyObject) -> Option<usize> {
    let text = item_text(item)?;
    rows.iter().position(|row| lossy(&row.token) == text)
}

fn row_for<'a>(rows: &'a [Row], item: &AnyObject) -> Option<&'a Row> {
    row_index(rows, item).map(|index| &rows[index])
}

fn item_text(item: &AnyObject) -> Option<String> {
    let retained = unsafe { Retained::retain(std::ptr::from_ref(item).cast_mut())? };
    let text = retained.downcast::<NSString>().ok()?;
    Some(lossy(&text))
}
