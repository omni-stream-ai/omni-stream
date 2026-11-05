#[path = "../mock_model.rs"]
mod mock_model;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let file = args
        .get(1)
        .ok_or("usage: omnistream-mock-model scenario.json [127.0.0.1:4320]")?;
    let steps = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    let server = mock_model::MockServer::start(
        args.get(2).map(String::as_str).unwrap_or("127.0.0.1:4320"),
        steps,
    )?;
    println!(
        "Mock model listening on {}. Press Enter to stop and print captured requests.",
        server.url
    );
    let _ = std::io::stdin().read_line(&mut String::new());
    println!(
        "{}",
        serde_json::to_string_pretty(&*server.requests.lock().unwrap())?
    );
    Ok(())
}
