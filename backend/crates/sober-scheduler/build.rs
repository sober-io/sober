fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::compile_protos("../../../shared/proto/sober/scheduler/v1/scheduler.proto")?;
    Ok(())
}
