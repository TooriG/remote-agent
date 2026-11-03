use uniffi_bindgen::bindings::{GenerateOptions, TargetLanguage, generate};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let (Some(library), Some(config), Some(output), None) = (
        arguments.next(),
        arguments.next(),
        arguments.next(),
        arguments.next(),
    ) else {
        return Err("usage: agent-bindgen library config output-directory".into());
    };
    generate(GenerateOptions {
        languages: vec![TargetLanguage::Swift, TargetLanguage::Kotlin],
        source: library.into(),
        config_override: Some(config.into()),
        out_dir: output.into(),
        ..Default::default()
    })?;
    Ok(())
}
