use evaluate::datasets::load_longmemeval_dataset;

fn main() -> anyhow::Result<()> {
    let path = "data/longmemeval_s_cleaned.json";
    let dataset = load_longmemeval_dataset(path)?;

    for entry in &dataset {
        println!("Question ID: {}", entry.question_id);
        println!("Question Type: {}", entry.question_type);
        println!("Answer: {}", entry.answer.as_string());

        if let Some(session) = entry.sessions()?.first().copied() {
            println!("First Session ID: {}", session.session_id);
            println!("First Session Date: {}", session.date);
        }
    }

    Ok(())
}
