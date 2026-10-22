//! `bimcraft-cli`: BIMCraft from the command line. Scripts, conversion, and an MCP server for
//! agents, all through the same commands.

mod mcp;
mod revit;

use std::io::Write;
use std::process::ExitCode;

use bimcraft_core::engine::{Session, catalog};
use bimcraft_core::sample::SAMPLE;
use serde_json::{Value, json};

const USAGE: &str = "USAGE:
  bimcraft-cli run [FILE] [--sample] [--cmd 'ID {json}']… [--script FILE]… [--save PATH]…
      Open FILE (a .bimcraft project, or an .ifc file to import) or start empty, then in order: build the sample house, run
      commands and scripts (one `ID {json}` per line), save. --save writes by extension:
      .bimcraft (the project), .ifc (IFC4), .svg or .png (the lowest level's plan).
  bimcraft-cli info FILE         levels, element counts, types and warnings of a project or an IFC file; of a Revit .rvt or
                                 .rfa file, its release, last save, levels and element counts by category
  bimcraft-cli convert IN OUT    a project or an IFC file to .ifc, .svg, .png or .bimcraft
  bimcraft-cli commands          every command and its parameters
  bimcraft-cli mcp               an MCP server on stdio, for agents
  bimcraft-cli --version         the release";

fn print(out: &mut impl Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{v}").map_err(|e| format!("stdout: {e}"))
}

fn print_pretty(out: &mut impl Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{}", serde_json::to_string_pretty(v).unwrap_or_default()).map_err(|e| format!("stdout: {e}"))
}

fn cli(args: &[String], out: &mut impl Write) -> Result<(), String> {
    let mut s = Session::new();
    match args {
        [cmd, rest @ ..] if cmd == "run" => {
            let mut rest = rest.iter();
            while let Some(arg) = rest.next() {
                let mut value = || rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"));
                let results = match arg.as_str() {
                    "--sample" => s.script(SAMPLE).map(|_| Vec::new())?,
                    "--cmd" => s.script(value()?)?,
                    "--script" => {
                        let path = value()?;
                        s.script(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?)?
                    }
                    "--save" => vec![s.execute("save", &json!({ "path": value()? }))?],
                    flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
                    file if revit::is_revit(file) => return Err(format!("{file}: {}", revit::NOT_YET)),
                    file => vec![s.execute("open", &json!({ "path": file }))?],
                };
                for r in results.iter().filter(|r| !r.is_null()) {
                    print(out, r)?;
                }
            }
            Ok(())
        }
        [cmd, file] if cmd == "info" && revit::is_revit(file) => print_pretty(out, &revit::info(file)?),
        [cmd, file] if cmd == "info" => {
            s.execute("open", &json!({ "path": file }))?;
            print_pretty(out, &s.execute("inspect", &json!({}))?)
        }
        [cmd, from, _] if cmd == "convert" && revit::is_revit(from) => Err(format!("{from}: {}", revit::NOT_YET)),
        [cmd, from, to] if cmd == "convert" => {
            s.execute("open", &json!({ "path": from }))?;
            print(out, &s.execute("save", &json!({ "path": to }))?)
        }
        [cmd] if cmd == "commands" => print_pretty(out, &catalog()),
        [flag] if flag == "--version" || flag == "-V" => {
            writeln!(out, "bimcraft-cli {}", env!("CARGO_PKG_VERSION")).map_err(|e| format!("stdout: {e}"))
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "mcp") {
        return match mcp::Server::new().serve(std::io::stdin().lock(), std::io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("bimcraft-cli mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match cli(&args, &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn cli_out(args: &[&str]) -> Result<String, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        cli(&args, &mut out).map(|()| String::from_utf8(out).unwrap())
    }

    #[test]
    fn run_builds_the_sample_and_saves_each_format() {
        let base = std::env::temp_dir().join(format!("bimcraft-cli-{}", std::process::id()));
        let path = |ext: &str| base.with_extension(ext).to_string_lossy().into_owned();
        let (project, ifc, png, svg) = (path("bimcraft"), path("ifc"), path("png"), path("svg"));
        let roof = r#"level {"elevation": 3200, "name": "Roof"}"#;
        let out = cli_out(&["run", "--sample", "--cmd", roof, "--save", &project, "--save", &ifc, "--save", &png]).unwrap();
        assert!(out.contains("\"Roof\""), "{out}");
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        assert!(std::fs::read_to_string(&ifc).unwrap().contains("IFCWALL("));
        let info: Value = serde_json::from_str(&cli_out(&["info", &project]).unwrap()).unwrap();
        assert_eq!(info["counts"]["room"], 4);
        assert_eq!(info["levels"][2]["name"], "Roof", "levels sort by elevation");
        cli_out(&["convert", &project, &svg]).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
        for p in [project, ifc, png, svg] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn bad_usage_and_failing_commands_are_errors() {
        assert!(cli_out(&[]).unwrap_err().contains("USAGE"));
        assert!(cli_out(&["run", "--frobnicate"]).unwrap_err().contains("--frobnicate"));
        assert!(cli_out(&["run", "--save"]).unwrap_err().contains("needs"));
        assert!(cli_out(&["run", "--cmd", r#"door {"at": [0, 0]}"#]).unwrap_err().contains("no wall"));
        assert!(cli_out(&["commands"]).unwrap().contains("\"wall\""));
    }

    #[test]
    fn version_names_the_release() {
        let line = format!("bimcraft-cli {}\n", env!("CARGO_PKG_VERSION"));
        assert_eq!(cli_out(&["--version"]).unwrap(), line);
        assert_eq!(cli_out(&["-V"]).unwrap(), line);
        assert!(cli_out(&["--version", "x"]).unwrap_err().contains("USAGE"));
    }

    /// A synthetic Revit project on disk, as a test file name in a folder of its own.
    #[cfg(feature = "rvt")]
    fn revit_file(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-rvt-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn info_of_a_revit_file_shows_the_release_the_last_save_the_levels_and_the_counts() {
        use bimcraft_rvt::project_fixture::{Lev, Project, Rec};
        let mut p = Project::new(2024);
        p.saved_by = "jdoe".into();
        p.recs = vec![Rec::new(100, -2_000_011), Rec::new(101, -2_000_011), Rec::new(200, -2_000_023), Rec::new(300, -2_009_999)];
        p.levels = vec![
            Lev::new(11, "Level 2", 9.84251968503937),
            Lev { story: Some(true), ..Lev::new(10, "Level 1", 0.0) },
            Lev { story: Some(false), owner: Some(4321), ..Lev::new(12, "Inside a family", 4.0) },
        ];
        let path = revit_file("house.rvt", &p.file());
        let info: Value = serde_json::from_str(&cli_out(&["info", &path]).unwrap()).unwrap();
        assert_eq!(info["revit"]["release"], 2024);
        assert_eq!(info["revit"]["worksharing"], "Not enabled");
        assert_eq!(info["saved_by"], "jdoe");
        assert!(info["saved_to"].as_str().unwrap().ends_with("house.rvt"));
        assert_eq!(info["elements"]["levels"][0]["name"], "Level 1");
        assert_eq!(info["elements"]["levels"][1]["elevation"], 3000.0, "millimetres");
        assert_eq!(info["elements"]["levels"][0]["building_story"], true);
        assert!(info["elements"]["levels"][1]["building_story"].is_null(), "a block with no setting says none");
        assert_eq!(info["elements"]["levels"].as_array().unwrap().len(), 2, "a level inside another element is not one");
        assert_eq!(info["elements"]["owned_levels"], json!([12]));
        let cat = |id: i64| info["elements"]["categories"].as_array().unwrap().iter().find(|c| c["category"] == id).cloned();
        assert_eq!(cat(-2_000_011).unwrap()["name"], "Walls");
        assert_eq!(cat(-2_000_011).unwrap()["elements"], 2);
        assert_eq!(cat(-2_000_240).unwrap()["elements"], 3, "the levels are elements too");
        assert!(cat(-2_009_999).unwrap()["name"].is_null(), "a category with no known name has none");
        assert_eq!(info["elements"]["total"], 7);
        // Nothing makes a project of it: run and convert say so and write nothing.
        let out = format!("{path}.bimcraft");
        for args in [vec!["run", &path], vec!["convert", &path, &out]] {
            let e = cli_out(&args).unwrap_err();
            assert!(e.contains("Revit file") && !std::path::Path::new(&out).exists(), "{e}");
        }
        // The saver's name and the path stay out of everything but the report.
        assert!(!std::path::Path::new(&out).exists());
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn info_of_a_file_that_is_not_a_project_or_not_revit_says_what_is_wrong() {
        let not = revit_file("fake.rvt", b"this is not a Revit file at all");
        let e = cli_out(&["info", &not]).unwrap_err();
        assert!(e.contains("fake.rvt") && e.contains("Compound File"), "{e}");
        // A file with the metadata and no partition (a family, say) still shows what it has, and says why it has no elements.
        let bytes = bimcraft_rvt::fixture::container(&[(
            "BasicFileInfo",
            &bimcraft_rvt::fixture::basic_file_info(&bimcraft_rvt::fixture::Basic::default()),
        )]);
        let family = revit_file("chair.rfa", &bytes);
        let info: Value = serde_json::from_str(&cli_out(&["info", &family]).unwrap()).unwrap();
        assert_eq!(info["revit"]["release"], 2024);
        assert!(info["elements"]["error"].as_str().unwrap().contains("Formats/Latest"), "{info}");
        assert!(cli_out(&["info", "/no/such/file.rvt"]).unwrap_err().contains("/no/such/file.rvt"));
    }

    #[cfg(not(feature = "rvt"))]
    #[test]
    fn a_build_without_revit_support_says_so() {
        assert!(cli_out(&["info", "x.rvt"]).unwrap_err().contains("without Revit support"));
    }
}
