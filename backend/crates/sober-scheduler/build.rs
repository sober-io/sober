use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../shared/proto/sober/scheduler/v1/scheduler.proto");
    tonic_prost_build::compile_protos(proto)?;
    Ok(())
}
