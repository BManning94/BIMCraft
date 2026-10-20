//! The discipline tabs after Structure: Plumbing, and the Electrical, Lightning Protection and Data tabs to come. A run (a
//! pipe, a conduit) is `Tool::Run` and a standing thing `Tool::Stand`, which the Structure tab built; what these tabs
//! place besides is a fixture or a device, one tool (`Tool::Fixture`) that places the families of the kinds it names. A tab adds
//! a ribbon panel and rows to the tables here, not a tool.

use bimcraft_core::family::Category;
use egui::Ui;

use crate::structure::same_tool;
use crate::style::Icon;
use crate::{BimApp, Tool, big_button, panel};

/// A fixture tool: the families of `kinds` are what it places, listed in its Type Selector, turned by Space.
pub(crate) struct FixtureTool {
    /// What `Tool::Fixture` names it by.
    pub key: &'static str,
    pub kinds: &'static [Category],
    /// Its two-letter shortcut, its button, its contextual tab's title and its tooltip.
    pub letters: &'static str,
    pub label: &'static str,
    pub title: &'static str,
    pub tip: &'static str,
    pub icon: Icon,
}

pub(crate) const FIXTURES: &[FixtureTool] = &[
    FixtureTool {
        key: "plumbing",
        kinds: &[Category::Plumbing, Category::Drain],
        letters: "PX",
        label: "Plumbing Fixture",
        title: "Place Plumbing Fixture",
        tip: "Plumbing Fixture (PX): a basin, a WC, a sink, a shower or a drain, where you click; Space turns it",
        icon: Icon::Fixture,
    },
    FixtureTool {
        key: "fitting",
        kinds: &[Category::PipeFitting],
        letters: "PF",
        label: "Pipe Fitting",
        title: "Place Pipe Fitting",
        tip: "Pipe Fitting (PF): an elbow or a tee, where you click on a pipe's height; Space turns it",
        icon: Icon::Fitting,
    },
    FixtureTool {
        key: "accessory",
        kinds: &[Category::PipeAccessory],
        letters: "PA",
        label: "Pipe Accessory",
        title: "Place Pipe Accessory",
        tip: "Pipe Accessory (PA): a valve, where you click on a pipe's height; Space turns it",
        icon: Icon::Accessory,
    },
];

pub(crate) fn fixture(key: &str) -> Option<&'static FixtureTool> {
    FIXTURES.iter().find(|f| f.key == key)
}

/// The fixture tool a two-letter shortcut picks.
pub(crate) fn fixture_with(letters: &str) -> Option<&'static FixtureTool> {
    FIXTURES.iter().find(|f| f.letters == letters)
}

/// The kinds of family the fixture tool `key` places.
pub(crate) fn kinds(key: &str) -> &'static [Category] {
    fixture(key).map_or(&[], |f| f.kinds)
}

impl BimApp {
    /// A panel's buttons: icon, label, tooltip and the tool each picks.
    fn buttons(&mut self, ui: &mut Ui, tools: &[(Icon, &str, &str, Tool)]) {
        for (icon, label, tip, tool) in tools {
            if big_button(ui, *icon, label, same_tool(&self.tool, tool), tip).clicked() {
                self.set_tool(tool.clone());
            }
        }
    }

    /// The button of the fixture tool `key`.
    fn fixture_button(key: &'static str) -> (Icon, &'static str, &'static str, Tool) {
        let f = fixture(key);
        (f.map_or(Icon::Component, |f| f.icon), f.map_or("", |f| f.label), f.map_or("", |f| f.tip), Tool::Fixture(key))
    }

    /// The Plumbing tab: Revit's Plumbing & Piping panel, a pipe, the fixtures that go on it and what joins and stops it.
    pub(crate) fn plumbing_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, "Plumbing & Piping", |ui| {
            let pipe = (
                Icon::Pipe,
                "Pipe",
                "Pipe (PI): click its start and its end; a chain of clicks makes a pipe for each step",
                Tool::Run("pipe", None),
            );
            self.buttons(ui, &[pipe, Self::fixture_button("plumbing"), Self::fixture_button("fitting"), Self::fixture_button("accessory")]);
        });
    }
}
