#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

fn main() {
    #[cfg(target_os = "windows")]
    {
        // Compile HLSL shaders
        #[cfg(not(debug_assertions))]
        compile_shaders();
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
mod shader_compilation {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        process::{self, Command},
    };

    pub fn compile_shaders() {
        let source = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
        for name in [
            "shaders.hlsl",
            "color_text_raster.hlsl",
            "alpha_correction.hlsl",
        ] {
            println!("cargo:rerun-if-changed={}", source.join(name).display());
        }
        let shader_path = source.join("shaders.hlsl");
        let out_dir = std::env::var("OUT_DIR").unwrap();

        let fxc_path = pinned_fxc_compiler();

        // Define all modules
        let modules = [
            "quad",
            "shadow",
            "path_rasterization",
            "path_sprite",
            "underline",
            "monochrome_sprite",
            "subpixel_sprite",
            "polychrome_sprite",
        ];

        let rust_binding_path = format!("{}/shaders_bytes.rs", out_dir);
        if Path::new(&rust_binding_path).exists() {
            fs::remove_file(&rust_binding_path)
                .expect("Failed to remove existing Rust binding file");
        }
        for module in modules {
            compile_shader_for_module(
                module,
                &out_dir,
                &fxc_path,
                shader_path.to_str().unwrap(),
                &rust_binding_path,
            );
        }

        let shader_path = source.join("color_text_raster.hlsl");
        compile_shader_for_module(
            "emoji_rasterization",
            &out_dir,
            &fxc_path,
            shader_path.to_str().unwrap(),
            &rust_binding_path,
        );
    }

    fn pinned_fxc_compiler() -> String {
        let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/windows-fxc");
        for name in ["manifest.json", "prepare.ps1"] {
            println!("cargo:rerun-if-changed={}", tools.join(name).display());
        }
        let output = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(tools.join("prepare.ps1"))
            .output()
            .expect("could not prepare the pinned Windows shader compiler");
        assert!(
            output.status.success(),
            "pinned Windows shader compiler setup failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("compiler path must be UTF-8")
            .trim()
            .to_owned()
    }

    fn compile_shader_for_module(
        module: &str,
        out_dir: &str,
        fxc_path: &str,
        shader_path: &str,
        rust_binding_path: &str,
    ) {
        // Compile vertex shader
        let output_file = format!("{}/{}_vs.h", out_dir, module);
        let const_name = format!("{}_VERTEX_BYTES", module.to_uppercase());
        compile_shader_impl(
            fxc_path,
            &format!("{module}_vertex"),
            &output_file,
            &const_name,
            shader_path,
            "vs_4_1",
        );
        generate_rust_binding(&const_name, &output_file, rust_binding_path);

        // Compile fragment shader
        let output_file = format!("{}/{}_ps.h", out_dir, module);
        let const_name = format!("{}_FRAGMENT_BYTES", module.to_uppercase());
        compile_shader_impl(
            fxc_path,
            &format!("{module}_fragment"),
            &output_file,
            &const_name,
            shader_path,
            "ps_4_1",
        );
        generate_rust_binding(&const_name, &output_file, rust_binding_path);
    }

    fn compile_shader_impl(
        fxc_path: &str,
        entry_point: &str,
        output_path: &str,
        var_name: &str,
        shader_path: &str,
        target: &str,
    ) {
        let output = Command::new(fxc_path)
            .args([
                "/T",
                target,
                "/E",
                entry_point,
                "/Fh",
                output_path,
                "/Vn",
                var_name,
                "/O3",
                shader_path,
            ])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    return;
                }
                println!(
                    "cargo::error=Shader compilation failed for {}:\n{}",
                    entry_point,
                    String::from_utf8_lossy(&result.stderr)
                );
                process::exit(1);
            }
            Err(e) => {
                println!("cargo::error=Failed to run fxc for {}: {}", entry_point, e);
                process::exit(1);
            }
        }
    }

    fn generate_rust_binding(const_name: &str, head_file: &str, output_path: &str) {
        let header_content = fs::read_to_string(head_file).expect("Failed to read header file");
        let const_definition = {
            let global_var_start = header_content.find("const BYTE").unwrap();
            let global_var = &header_content[global_var_start..];
            let equal = global_var.find('=').unwrap();
            global_var[equal + 1..].trim()
        };
        let rust_binding = format!(
            "const {}: &[u8] = &{}\n",
            const_name,
            const_definition.replace('{', "[").replace('}', "]")
        );
        let mut options = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output_path)
            .expect("Failed to open Rust binding file");
        options
            .write_all(rust_binding.as_bytes())
            .expect("Failed to write Rust binding file");
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
use shader_compilation::compile_shaders;
