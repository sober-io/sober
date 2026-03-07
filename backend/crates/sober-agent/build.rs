fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::compile_protos("../../../shared/proto/sober/agent/v1/agent.proto")?;
    Ok(())
}
