/// Example demonstrating how to use the Models API
///
/// This example shows how to:
/// - List available models with pagination
/// - Retrieve information about a specific model
use claudius::{Anthropic, ModelListParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the client
    let client = Anthropic::new(None)?;

    // List all models
    println!("Listing all available models:");
    let models_response = client.list_models(None).await?;
    for model in models_response.models() {
        println!("- {} ({})", model.display_name, model.id);
    }

    // List models with pagination
    println!("\nListing first 5 models:");
    let params = ModelListParams::new().with_limit(5);
    let models_response = client.list_models(Some(params)).await?;

    for model in models_response.models() {
        println!("- {} ({})", model.display_name, model.id);
    }

    if models_response.has_more() {
        println!("There are more models available. Use pagination to fetch them.");
        if let Some(last_id) = models_response.last_id() {
            println!("To get the next page, use after_id: {last_id}");
        }
    }

    // Get information about a specific model
    if let Some(model) = models_response.models().first() {
        println!("\nRetrieving details for model: {}", model.id);
        let model_info = client.get_model(&model.id).await?;
        println!("Model ID: {}", model_info.id);
        println!("Display Name: {}", model_info.display_name);
        println!("Created At: {}", model_info.created_at);
        println!("Type: {:?}", model_info.r#type);
    }

    Ok(())
}
