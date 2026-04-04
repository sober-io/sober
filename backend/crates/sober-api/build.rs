fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::compile_protos("../../proto/sober/agent/v1/agent.proto")?;
    tonic_prost_build::compile_protos("../../proto/sober/gateway/v1/gateway.proto")?;
    Ok(())
}
