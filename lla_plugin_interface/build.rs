use std::io::Result;

fn main() -> Result<()> {
    #[cfg(feature = "regenerate-protobuf")]
    {
        use std::path::PathBuf;
        use std::process::Command;
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        println!("cargo:rerun-if-changed=src/plugin.proto");

        if std::env::var("PROTOC").is_ok()
            || Command::new("protoc").output().is_ok()
            || Command::new("protoc-gen-rust").output().is_ok()
            || Command::new("protoc-gen-rust-macos").output().is_ok()
            || Command::new("protoc-gen-rust-linux").output().is_ok()
        {
            if let Err(e) = prost_build::Config::new()
                .out_dir(&out_dir)
                .compile_protos(&["src/plugin.proto"], &["src/"])
            {
                eprintln!("Warning: Failed to compile protos: {}", e);
                eprintln!("Using pre-generated files from src/generated/");
                return Ok(());
            }

            if let Err(e) = std::fs::create_dir_all("src/generated") {
                eprintln!("Warning: Failed to create generated directory: {}", e);
                return Ok(());
            }

            if let Err(e) = std::fs::copy(out_dir.join("lla_plugin.rs"), "src/generated/mod.rs") {
                eprintln!("Warning: Failed to copy generated file: {}", e);
                eprintln!("Using pre-generated files from src/generated/");
                return Ok(());
            }
        } else {
            eprintln!("Note: protoc not found, using pre-generated files from src/generated/");
        }
    }
    Ok(())
}
