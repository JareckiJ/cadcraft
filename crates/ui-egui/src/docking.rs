//! Application panel identities and persistence around the shared docking renderer.
//! Document contents remain owned by their existing view implementations.

use craft_ui::docking::{
    Action, DockArea, DockCommand, DockOperation, DockStyle, FloatSpace, Layout, Location, Node, PanelLimits, Permissions, Placement, StackEntry,
    UiDock, Zone,
};
use craft_ui::layout::{SplitAxis, SplitSize};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Panel {
    Canvas,
    ToolSets,
    QuickCalc,
    Layers,
    Properties,
    #[serde(rename = "externalReferences")]
    ExternalRefs,
    /// The Block Manager as a panel.
    Blocks,
}

impl Panel {
    pub const ALL: &[Self] = &[Self::Canvas, Self::ToolSets, Self::Layers, Self::Properties, Self::ExternalRefs];
    pub fn id(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::ToolSets => "toolSets",
            Self::QuickCalc => "quickCalc",
            Self::Layers => "layers",
            Self::Properties => "properties",
            Self::ExternalRefs => "externalReferences",
            Self::Blocks => "blocks",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Canvas => "Drawing",
            Self::ToolSets => "Tool Sets",
            Self::QuickCalc => "QuickCalc",
            Self::Layers => "Layers",
            Self::Properties => "Properties",
            Self::ExternalRefs => "External References",
            Self::Blocks => "Blocks",
        }
    }
    /// Palettes outside the default arrangement, opened on request (QUICKCALC, BLOCKSPALETTE).
    pub const ON_DEMAND: &[Self] = &[Self::QuickCalc, Self::Blocks];
}

/// The drawing view never moves, floats, closes or takes tabs; palettes dock beside it.
pub fn permissions(panel: &Panel) -> Permissions {
    if *panel == Panel::Canvas { Permissions { accepts_splits: true, ..Permissions::PROTECTED } } else { Permissions::default() }
}

fn tabs(panel: Panel) -> Node<Panel> {
    Node::Tabs { panels: vec![panel], active: 0 }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<Panel>, second: Node<Panel>) -> Node<Panel> {
    Node::Split { axis, size, first: Box::new(first), second: Box::new(second) }
}

/// The saved panel arrangement: craft-ui's workspace over CADCraft's panels.
pub type Workspace = craft_ui::docking::Workspace<Panel>;

/// Tool Sets on the left, the drawing, and the Layers and Properties accordion on the right.
pub fn default_workspace() -> Workspace {
    Workspace::new(Layout {
        root: Some(split(
            SplitAxis::Horizontal,
            SplitSize::FixedFirst(220.0),
            tabs(Panel::ToolSets),
            split(
                SplitAxis::Horizontal,
                SplitSize::FixedSecond(300.0),
                tabs(Panel::Canvas),
                Node::Stack {
                    entries: vec![
                        StackEntry { panel: Panel::Layers, open: true, height: Some(280.0) },
                        StackEntry { panel: Panel::Properties, open: true, height: None },
                    ],
                },
            ),
        )),
        floating: Vec::new(),
    })
}

/// craft-ui's checks plus the drawing view's: a saved or hand-edited file cannot hide, float,
/// group or collapse it, or restore a palette over it.
pub fn validate(workspace: &Workspace) -> Result<(), String> {
    workspace.validate().map_err(|error| error.to_string())?;
    if !workspace.layout.contains(&Panel::Canvas) {
        return Err("The panel layout must contain its document view".into());
    }
    if workspace.layout.floating.iter().any(|window| window.contains(&Panel::Canvas)) {
        return Err("The document view cannot float".into());
    }
    let mut nodes: Vec<_> = workspace.layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => {
                nodes.push(first);
                nodes.push(second);
            }
            Node::Tabs { panels, .. } if panels.contains(&Panel::Canvas) && panels.len() != 1 => {
                return Err("The document view cannot share a tab group".into());
            }
            Node::Stack { entries } if entries.iter().any(|entry| entry.panel == Panel::Canvas) => {
                return Err("The document view cannot be collapsed".into());
            }
            Node::Dock { columns, .. } if columns.iter().flat_map(|column| &column.groups).any(|group| group.panels.contains(&Panel::Canvas)) => {
                return Err("The document view cannot be in a dock".into());
            }
            _ => {}
        }
    }
    let over_canvas = |location: &Location<Panel>| {
        location.anchor == Some(Panel::Canvas) && matches!(location.placement, Placement::Tab { .. } | Placement::Split(Zone::Center))
    };
    if workspace.hidden.iter().any(|(_, location)| over_canvas(location)) {
        return Err("A hidden panel cannot restore over the document view".into());
    }
    Ok(())
}

/// Show a panel and select it at its recorded place (craft-ui places a panel it has no place
/// for), keeping the drawing view's rules.
pub fn reveal_panel(workspace: &mut Workspace, panel: Panel) -> Result<(), String> {
    let mut next = workspace.clone();
    next.set_visible(panel, true, None, permissions).map_err(|error| error.to_string())?;
    validate(&next)?;
    *workspace = next;
    Ok(())
}

/// Show or hide a panel for the Tool Sets and palette switches. Showing a panel whose recorded
/// anchor is shown restores it there without changing which tab is selected or which accordion
/// entries are open. Otherwise it goes through [`reveal_panel`], which regroups it with hidden
/// panels it was beside, and selects it.
fn set_visible(workspace: &mut Workspace, panel: Panel, visible: bool) -> Result<(), String> {
    if workspace.layout.contains(&panel) == visible {
        return Ok(());
    }
    if !visible {
        return workspace.set_visible(panel, false, None, permissions).map_err(|error| error.to_string());
    }
    let mut next = workspace.clone();
    let location = next.hidden.iter().find(|(id, _)| *id == panel).map(|(_, location)| location.clone());
    // A direct restore against a hidden anchor would open a window of its own.
    let direct = location.filter(|location| location.anchor.is_none_or(|anchor| next.layout.contains(&anchor)));
    if direct.is_some_and(|location| next.layout.restore(panel, &location).is_ok()) {
        next.hidden.retain(|(id, _)| *id != panel);
        validate(&next)?;
        *workspace = next;
        return Ok(());
    }
    reveal_panel(workspace, panel)
}

/// `ui.dock` params: `{action}` or `{operation, …}` in craft-ui's shared grammar.
fn apply_command(workspace: &mut Workspace, params: &Value) -> Result<(), String> {
    validate(workspace)?;
    let operation = if let Some(action) = params.get("action") {
        UiDock::Action { action: serde_json::from_value(action.clone()).map_err(|error| error.to_string())? }
    } else {
        UiDock::Operation(serde_json::from_value(params.clone()).map_err(|error| error.to_string())?)
    };
    let mut next = workspace.clone();
    match operation {
        UiDock::Operation(DockOperation::Open { panel } | DockOperation::Activate { panel }) if !next.layout.contains(&panel) => {
            reveal_panel(&mut next, panel)?;
        }
        operation => {
            // Every ui.dock operation carries its own rect, so the float space is never consulted.
            let space = FloatSpace::new(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)));
            DockCommand::UiDock(operation).apply(&mut next, false, &space, permissions).map_err(|error| error.to_string())?;
        }
    }
    validate(&next)?;
    *workspace = next;
    Ok(())
}

fn sync(workspace: &mut Workspace, app: &crate::CadApp) -> Result<(), String> {
    // As a ribbon, Tool Sets sits under the menu bar, not among the palettes.
    set_visible(workspace, Panel::ToolSets, app.ui.show_toolsets && !app.ui.toolsets_ribbon)?;
    if !app.ui.show_palettes {
        set_visible(workspace, Panel::Layers, false)?;
        set_visible(workspace, Panel::Properties, false)?;
    } else if !workspace.layout.contains(&Panel::Layers) && !workspace.layout.contains(&Panel::Properties) {
        // Reopen the last surviving accordion entry before restoring its earlier sibling.
        set_visible(workspace, Panel::Properties, true)?;
        set_visible(workspace, Panel::Layers, true)?;
    }
    Ok(())
}

fn publish_visibility(app: &mut crate::CadApp, workspace: &Workspace) {
    if !app.ui.toolsets_ribbon {
        app.ui.show_toolsets = workspace.layout.contains(&Panel::ToolSets);
    }
    app.ui.show_palettes = workspace.layout.contains(&Panel::Layers) || workspace.layout.contains(&Panel::Properties);
}

pub fn command(app: &mut crate::CadApp, reset: bool, params: &Value) -> Result<Value, String> {
    let mut workspace = app.ui.docking.clone();
    if reset {
        workspace = default_workspace();
    } else {
        sync(&mut workspace, app)?;
        apply_command(&mut workspace, params)?;
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    Ok(json!({"docking": app.ui.docking}))
}

/// Validate and repair the stored workspace, sync it with the app, and render the dock panels; a failed restore falls back to the default layout with a status message.
pub fn show(app: &mut crate::CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let mut workspace = app.ui.docking.clone();
    if let Err(error) = validate(&workspace).and_then(|()| sync(&mut workspace, app)) {
        app.set_status(format!("Panel layout could not be restored: {error}"));
        workspace = default_workspace();
        let _ = sync(&mut workspace, app);
    }
    // Bodies can dispatch commands (including the drawing's typed command line).
    // Keep the canonical workspace accessible while rendering an immutable snapshot.
    app.ui.docking = workspace.clone();
    let mut style = DockStyle::from_ui(ui);
    style.min_pane = 96.0;
    style.background = t.panel;
    style.tab_background = t.chrome;
    style.active_background = t.tab_active;
    style.text = t.text;
    style.inactive_text = t.text_dim;
    style.border = egui::Stroke::new(1.0, t.border);
    style.accent = t.accent;
    style.font = crate::theme::body();
    // Panels arrange in a split tree around the drawing; floating title bars only move windows.
    style.new_dock_zones = false;
    style.title_bar_docks = false;
    let output = DockArea::new(egui::Id::unique("cadcraft-docking")).show_with_limits(
        ui,
        &workspace.layout,
        &style,
        |panel| crate::i18n::t(panel.label()).into(),
        permissions,
        panel_limits,
        |ui, panel| match panel {
            Panel::Canvas => {
                ui.painter().rect_filled(ui.max_rect(), 0.0, t.canvas);
                crate::canvas::show(app, ui);
            }
            Panel::ToolSets => crate::palettes::toolsets(app, ui),
            Panel::QuickCalc => crate::palettes::quickcalc::show(app, ui),
            Panel::Blocks => crate::blockmgr::panel(app, ui),
            Panel::Layers => {
                egui::ScrollArea::vertical()
                    .id_salt("dock-layers")
                    .auto_shrink([false, false])
                    .show(ui, |ui| crate::palettes::layers_section(app, ui));
            }
            Panel::Properties => {
                egui::ScrollArea::vertical().id_salt("dock-properties").auto_shrink([false, false]).show(ui, |ui| {
                    crate::shortcut_menus::palette(app, ui, crate::palettes::properties_section, crate::shortcut_menus::properties_entries)
                });
            }
            Panel::ExternalRefs => {
                egui::ScrollArea::both().id_salt("dock-xrefs").auto_shrink([false, false]).show(ui, |ui| crate::xrefs::section(app, ui));
            }
        },
    );
    // Existing panel close buttons and commands still own their visibility switches.
    let rendered = workspace;
    let mut workspace = app.ui.docking.clone();
    let _ = sync(&mut workspace, app);
    let stale = workspace != rendered;
    if stale {
        craft_ui::docking::cancel_drag::<Panel>(ui.ctx(), egui::Id::unique("cadcraft-docking"));
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    if stale {
        return;
    }
    for action in output.actions {
        // Tool Sets dragged to the top edge becomes the ribbon under the menu bar.
        if let Action::Move { panel: Panel::ToolSets, anchor: Panel::Canvas, placement: Placement::Split(Zone::Top) } = &action {
            if let Err(error) = apply_toolsets(app, ToolSetsAction::Ribbon, ui.ctx()) {
                app.set_status(format!("Panel move was not applied: {error}"));
            }
            continue;
        }
        if let Err(error) =
            serde_json::to_value(action).map_err(|error| error.to_string()).and_then(|action| command(app, false, &json!({"action": action})))
        {
            app.set_status(format!("Panel move was not applied: {error}"));
        }
    }
}

fn panel_limits(panel: &Panel) -> PanelLimits {
    let (min, max_width) = match panel {
        Panel::Canvas => (egui::vec2(300.0, 180.0), 1_000_000.0),
        Panel::QuickCalc => (egui::vec2(240.0, 160.0), 600.0),
        Panel::Blocks => (egui::vec2(240.0, 200.0), 600.0),
        Panel::ToolSets => (egui::vec2(220.0, 150.0), 480.0),
        Panel::Layers | Panel::Properties | Panel::ExternalRefs => (egui::vec2(220.0, 100.0), 720.0),
    };
    PanelLimits { min, max: egui::vec2(max_width, 1_000_000.0) }
}

/// Where the Tool Sets palette is: shown as a ribbon under the menu bar, floating, pinned in a
/// column of its own (left of the drawing by default), in a group with other palettes, or closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSetsPlace {
    Ribbon,
    Floating,
    Pinned,
    Grouped,
    Closed,
}

/// What the Tool Sets right-click menu asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSetsAction {
    Float,
    PinLeft,
    Ribbon,
    /// Into the group (tabs or stack) that holds this panel.
    Group(Panel),
    Close,
}

/// Every panel of a node, in order.
fn node_panels(node: &Node<Panel>, out: &mut Vec<Panel>) {
    match node {
        Node::Split { first, second, .. } => {
            node_panels(first, out);
            node_panels(second, out);
        }
        Node::Tabs { panels, .. } => out.extend(panels.iter().copied()),
        Node::Stack { entries } => out.extend(entries.iter().map(|e| e.panel)),
        Node::Dock { columns, .. } => {
            for g in columns.iter().flat_map(|c| &c.groups) {
                out.extend(g.panels.iter().copied());
            }
        }
        _ => {}
    }
}

/// The groups (tab groups, stacks and floating groups) of a layout, each as its panels.
fn groups(layout: &Layout<Panel>) -> Vec<Vec<Panel>> {
    fn walk(node: &Node<Panel>, out: &mut Vec<Vec<Panel>>) {
        match node {
            Node::Split { first, second, .. } => {
                walk(first, out);
                walk(second, out);
            }
            Node::Tabs { panels, .. } => out.push(panels.clone()),
            Node::Stack { entries } => out.push(entries.iter().map(|e| e.panel).collect()),
            Node::Dock { columns, .. } => out.extend(columns.iter().flat_map(|c| &c.groups).map(|g| g.panels.clone())),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(root) = &layout.root {
        walk(root, &mut out);
    }
    out.extend(layout.floating.iter().map(|g| g.panels().copied().collect::<Vec<_>>()));
    out
}

pub fn toolsets_place(app: &crate::CadApp) -> ToolSetsPlace {
    if !app.ui.show_toolsets {
        return ToolSetsPlace::Closed;
    }
    if app.ui.toolsets_ribbon {
        return ToolSetsPlace::Ribbon;
    }
    let layout = &app.ui.docking.layout;
    if layout.floating.iter().any(|g| g.panels().any(|p| *p == Panel::ToolSets)) {
        return ToolSetsPlace::Floating;
    }
    match groups(layout).into_iter().find(|g| g.contains(&Panel::ToolSets)) {
        Some(g) if g.len() > 1 => ToolSetsPlace::Grouped,
        _ => ToolSetsPlace::Pinned,
    }
}

/// The groups Tool Sets can join: (a member to join beside, the group's name).
pub fn toolsets_groups(app: &crate::CadApp) -> Vec<(Panel, String)> {
    groups(&app.ui.docking.layout)
        .into_iter()
        .filter(|g| !g.contains(&Panel::ToolSets) && !g.contains(&Panel::Canvas))
        .filter_map(|g| {
            let first = *g.first()?;
            let name = g.iter().map(|p| crate::i18n::t(p.label()).to_string()).collect::<Vec<_>>().join(" / ");
            Some((first, name))
        })
        .collect()
}

/// The Tool Sets right-click menu: every place but the one it is in now.
pub fn toolsets_menu(ui: &mut egui::Ui, place: ToolSetsPlace, groups: &[(Panel, String)], out: &mut Option<ToolSetsAction>) {
    let item = |ui: &mut egui::Ui, label: &str, action: ToolSetsAction, out: &mut Option<ToolSetsAction>| {
        if ui.button(crate::i18n::t(label)).clicked() {
            *out = Some(action);
            ui.close();
        }
    };
    if place != ToolSetsPlace::Floating {
        item(ui, "Float Panel", ToolSetsAction::Float, out);
    }
    if place != ToolSetsPlace::Pinned {
        item(ui, "Pin to Left", ToolSetsAction::PinLeft, out);
    }
    if place != ToolSetsPlace::Ribbon {
        item(ui, "Transform to Ribbon", ToolSetsAction::Ribbon, out);
    }
    if !groups.is_empty() {
        ui.menu_button(crate::i18n::t("Move to Group"), |ui| {
            for (panel, name) in groups {
                if ui.button(crate::i18n::widget(ui, name, egui::TextStyle::Button)).clicked() {
                    *out = Some(ToolSetsAction::Group(*panel));
                    ui.close();
                }
            }
        });
    }
    item(ui, "Close Panel", ToolSetsAction::Close, out);
}

/// Width of Tool Sets pinned on the left.
const PINNED_WIDTH: f32 = 220.0;

/// Carry out a Tool Sets menu choice.
pub fn apply_toolsets(app: &mut crate::CadApp, action: ToolSetsAction, ctx: &egui::Context) -> Result<(), String> {
    if action == ToolSetsAction::Close {
        app.ui.show_toolsets = false;
        return Ok(());
    }
    app.ui.show_toolsets = true;
    app.ui.toolsets_ribbon = action == ToolSetsAction::Ribbon;
    let mut workspace = app.ui.docking.clone();
    sync(&mut workspace, app)?;
    match action {
        ToolSetsAction::Ribbon | ToolSetsAction::Close => {}
        ToolSetsAction::Float => {
            let screen = ctx.content_rect();
            let rect = [screen.left() + 60.0, screen.top() + 140.0, 240.0, (screen.height() - 220.0).clamp(240.0, 640.0)];
            workspace.apply(Action::Float { panel: Panel::ToolSets, rect }, permissions).map_err(|e| e.to_string())?;
        }
        ToolSetsAction::Group(anchor) => {
            workspace
                .apply(Action::Move { panel: Panel::ToolSets, anchor, placement: Placement::Split(Zone::Center) }, permissions)
                .map_err(|e| e.to_string())?;
        }
        ToolSetsAction::PinLeft => {
            // Out of where it is, then back in as a column left of everything docked.
            workspace.apply(Action::Close { panel: Panel::ToolSets }, permissions).map_err(|e| e.to_string())?;
            let mut sibling = Vec::new();
            if let Some(root) = &workspace.layout.root {
                node_panels(root, &mut sibling);
            }
            let location = Location {
                floating: None,
                anchor: Some(Panel::Canvas),
                placement: Placement::Split(Zone::Left),
                size: Some(SplitSize::FixedFirst(PINNED_WIDTH)),
                stack: None,
                sibling,
                group: None,
            };
            workspace.hidden.retain(|(p, _)| *p != Panel::ToolSets);
            workspace.hidden.push((Panel::ToolSets, location));
            workspace.set_visible(Panel::ToolSets, true, None, permissions).map_err(|e| e.to_string())?;
        }
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    Ok(())
}
