use std::io::Result;

fn main() -> Result<()> {
    #[cfg(feature = "regenerate-protobuf")]
    {
        use std::path::PathBuf;
        use std::process::Command;
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        println!("cargo:rerun-if-changed=src/plugin.proto");

        let protoc_available = std::env::var("PROTOC").is_ok()
            || Command::new("protoc")
                .output()
                .is_ok_and(|output| output.status.success());
        if !protoc_available {
            let protoc = protoc_bin_vendored::protoc_bin_path()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            std::env::set_var("PROTOC", protoc);
        }

        prost_build::Config::new()
            .out_dir(&out_dir)
            .compile_protos(&["src/plugin.proto"], &["src/"])
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        // Best-effort: update the checked-in bindings so users without protoc can still build.
        if let Err(e) = std::fs::create_dir_all("src/generated") {
            eprintln!("Warning: Failed to create generated directory: {}", e);
            return Ok(());
        }

        if let Err(e) = std::fs::copy(out_dir.join("lla_plugin.rs"), "src/generated/mod.rs") {
            eprintln!("Warning: Failed to copy generated file: {}", e);
        }
    }
    Ok(())
}
