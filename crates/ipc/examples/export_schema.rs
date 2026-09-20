fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol");
    for (name, schema) in [
        (
            "request.schema.json",
            schemars::schema_for!(savescummer_ipc::Request),
        ),
        (
            "response.schema.json",
            schemars::schema_for!(savescummer_ipc::Response),
        ),
    ] {
        std::fs::write(
            directory.join(name),
            format!("{}\n", serde_json::to_string_pretty(&schema)?),
        )?;
    }
    Ok(())
}
