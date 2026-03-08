use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../shared/proto/sober/agent/v1/agent.proto");
    tonic_prost_build::compile_protos(proto)?;
    Ok(())
}
