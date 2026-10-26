//! The Block Editor contextual Ribbon tab: it shows while BEDIT is open, as Windows CAD ribbons
//! do, and gives the previous tab back when the Block Editor closes. Panels, in the usual order:
//! Open/Save, Geometric, Dimensional, Manage, Action Parameters, Close (observed black-box; icons
//! are CADCraft's own). Commands CADCraft doesn't have yet show greyed out in their place.

use egui::Rect;

use crate::CadApp;
use crate::icons::Icon;
use crate::ribbon::{Action, Cell, DIMENSIONAL, GEOMETRIC, NONE, PanelDef, c, n};

/// The tab's id (as `UiState::ribbon_tab` holds it while shown) and English title.
pub const TAB_ID: &str = "blockeditor";
pub const TAB_TITLE: &str = "Block Editor";

const PANELS: &[PanelDef] = &[
    PanelDef {
        title: "Open/Save",
        cells: &[
            Cell::Big(c("bedit", "Edit Block", Icon::BlockEdit), NONE),
            Cell::Big(c("bsave", "Save Block", Icon::Save), NONE),
            Cell::Big(n("btestblock", "Test Block"), NONE),
        ],
        more: &[c("bsaveas", "Save Block As", Icon::SaveAs)],
    },
    GEOMETRIC,
    DIMENSIONAL,
    PanelDef {
        title: "Manage",
        cells: &[
            Cell::Small(&[
                (n("delconstraint", "Delete"), NONE),
                (n("bconstruction", "Construction"), NONE),
                (n("bconstatusmode", "Constraint Status"), NONE),
            ]),
            Cell::Big(n("ui.dialog.parameters", "Parameters Manager"), NONE),
            Cell::Big(n("bauthorpalette", "Authoring Palettes"), NONE),
        ],
        more: NONE,
    },
    PanelDef {
        title: "Action Parameters",
        cells: &[
            Cell::Big(c("bparameter", "Point", Icon::Point), NONE),
            Cell::Big(c("baction", "Move", Icon::Move), NONE),
            Cell::Big(c("attdef", "Attribute Definition", Icon::AttDef), NONE),
        ],
        more: NONE,
    },
    PanelDef { title: "Close", cells: &[Cell::Big(c("bclose", "Close Block Editor", Icon::CloseEditor), NONE)], more: NONE },
];

/// Whether the tab shows: the Block Editor is open.
pub fn tab(app: &CadApp) -> Option<&'static str> {
    app.session.state().ok()?.block_editor().map(|_| TAB_ID)
}

pub(crate) fn body(app: &mut CadApp, ui: &mut egui::Ui, body: Rect, out: &mut Option<Action>) {
    crate::ribbon::panels(app, ui, body, TAB_ID, PANELS, out);
}

#[cfg(test)]
mod tests {
    /// Saving and closing are real commands (not greyed out).
    #[test]
    fn save_and_close_exist() {
        for id in ["bedit", "bsave", "bsaveas", "bclose", "attdef"] {
            assert!(crate::ribbon::known(id), "{id} is not a command");
        }
    }
}
