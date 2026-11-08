//! `report.md`: a summary by screen, then each state's side-by-side image and
//! its differences, and every state that could not be compared, with why.
use super::{
    compare::Difference,
    map::{Map, Reference, State},
};

/// What became of one state.
pub struct Outcome<'a> {
    pub state: &'a State,
    pub result: Result<Vec<Difference>, String>,
}

pub fn render(run: &str, map: &Map, outcomes: &[Outcome]) -> String {
    let mut report = format!(
        "# NSH parity {run}\n\nNSH (left of each table, right of each image) against T3 Code rendered from its source by \
         `scripts/nsh-parity-t3.ts`, or, for the sidebar, the GPUI client's layout from its source. Both clients ran \
         against Bex's Codex fixture, at {}×{} points. Positions are in points from the state's column (or the element's \
         anchor) and the window's top; colours are sRGB. Tolerances: position {}, size {}, drawn text and glyphs {}, \
         colour distance {}, corner radius {}.\n\n## Summary\n\n| Screen | States compared | Not compared | Differences | Missing in NSH |\n|---|---|---|---|---|\n",
        map.viewport.width,
        map.viewport.height,
        map.tolerance.position,
        map.tolerance.size,
        map.tolerance.text,
        map.tolerance.color,
        map.tolerance.radius
    );
    for screen in screens(outcomes) {
        let of_screen: Vec<_> = outcomes
            .iter()
            .filter(|outcome| outcome.state.screen == screen)
            .collect();
        let differences: Vec<_> = of_screen
            .iter()
            .filter_map(|outcome| outcome.result.as_ref().ok())
            .flatten()
            .collect();
        report.push_str(&format!(
            "| {screen} | {} | {} | {} | {} |\n",
            of_screen
                .iter()
                .filter(|outcome| outcome.result.is_ok())
                .count(),
            of_screen
                .iter()
                .filter(|outcome| outcome.result.is_err())
                .count(),
            differences.len(),
            differences
                .iter()
                .filter(|difference| difference.nsh == "missing")
                .count(),
        ));
    }
    for screen in screens(outcomes) {
        report.push_str(&format!("\n## {screen}\n"));
        for outcome in outcomes
            .iter()
            .filter(|outcome| outcome.state.screen == screen)
        {
            report.push_str(&section(map, outcome));
        }
    }
    report
}

fn screens<'a>(outcomes: &'a [Outcome]) -> Vec<&'a str> {
    let mut screens: Vec<&str> = Vec::new();
    for outcome in outcomes {
        if !screens.contains(&outcome.state.screen.as_str()) {
            screens.push(&outcome.state.screen);
        }
    }
    screens
}

fn section(map: &Map, outcome: &Outcome) -> String {
    let state = outcome.state;
    let name = &state.name;
    let reference = match state.reference {
        Reference::T3 => "T3",
        Reference::Gpui => "GPUI",
    };
    let mut text = format!("\n### {name}\n\n{}\n\n", state.description);
    let differences = match &outcome.result {
        Err(reason) => {
            text.push_str(&format!("Not compared: {}\n", cell(reason)));
            return text;
        }
        Ok(differences) => differences,
    };
    text.push_str(&format!(
        "![{name}: {reference} left, NSH right; differing elements outlined]({name}/composite.png)\n\n"
    ));
    if differences.is_empty() {
        text.push_str("No differences beyond tolerance.\n");
        return text;
    }
    text.push_str(&format!(
        "| Element | Metric | {reference} | NSH |\n|---|---|---|---|\n"
    ));
    for difference in differences {
        let what = map
            .element(&difference.element)
            .map_or(difference.element.as_str(), |element| element.what.as_str());
        text.push_str(&format!(
            "| {} (`{}`) | {} | {} | {} |\n",
            cell(what),
            difference.element,
            difference.metric,
            cell(&difference.reference),
            cell(&difference.nsh)
        ));
    }
    text
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = r##"
[viewport]
width = 1280
height = 800

[tolerance]
position = 4
size = 4
text = 1
color = 24
radius = 2

[[state]]
name = "turn"
screen = "Conversation"
description = "A finished turn."
nsh = []
elements = ["row"]

[[state]]
name = "approval"
screen = "Conversation"
description = "An approval."
nsh = []
elements = ["row"]

[[state]]
name = "search"
screen = "Sidebar"
description = "The search field."
reference = "gpui"
nsh = []
elements = ["search"]

[[element]]
id = "row"
what = "Work row | folded"
kind = "text"
t3 = ".row"
nsh = { text = "Worked" }

[[element]]
id = "search"
what = "Search icon"
kind = "icon"
nsh = { text = "Search" }
gpui = { height = 1, source = "sidebar.rs" }
"##;

    #[test]
    fn the_report_summarises_by_screen_and_details_each_state() {
        let map = Map::parse(MAP).unwrap();
        let missing = Difference {
            element: "row".into(),
            metric: "present",
            reference: "shown".into(),
            nsh: "missing".into(),
        };
        let moved = Difference {
            element: "search".into(),
            metric: "glyph centre from its container's centre",
            reference: "0.0, 0.0".into(),
            nsh: "0.0, 1.5".into(),
        };
        let outcomes = [
            Outcome {
                state: &map.states[0],
                result: Ok(vec![missing]),
            },
            Outcome {
                state: &map.states[1],
                result: Err("the fixture Host did not\nbecome ready".into()),
            },
            Outcome {
                state: &map.states[2],
                result: Ok(vec![moved]),
            },
        ];
        let text = render("run-1", &map, &outcomes);
        for line in [
            "| Conversation | 1 | 1 | 1 | 1 |",
            "| Sidebar | 1 | 0 | 1 | 0 |",
            "![turn: T3 left, NSH right; differing elements outlined](turn/composite.png)",
            "| Work row \\| folded (`row`) | present | shown | missing |",
            "Not compared: the fixture Host did not become ready",
            "| Element | Metric | GPUI | NSH |",
            "| Search icon (`search`) | glyph centre from its container's centre | 0.0, 0.0 | 0.0, 1.5 |",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert!(text.find("## Conversation").unwrap() < text.find("## Sidebar").unwrap());
    }
}
