use evaluate::datasets::load_locomo_dataset;

fn main() -> anyhow::Result<()> {
    let dataset = load_locomo_dataset("data/locomo10.json")?;

    for entry in dataset {
        println!("ID: {}", entry.sample_id);
        println!("Speaker A: {}", entry.conversation.speaker_a);
        if let Some(first_session) = entry.conversation.ordered_sessions()?.first() {
            println!("First Session ID: {}", first_session.session_id);
            println!("First Session Date: {}", first_session.start_time);
        }
    }

    Ok(())
}
