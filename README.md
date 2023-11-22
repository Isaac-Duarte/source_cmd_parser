# Source Cmd Parser
This crate is a framework to externally add chat commands on the client side of Counter-Strike:Source

## How it works
In many source games, there is an ability to write the console output to a log file. This crate waits for changes in the log file and parses the output to find chat messages. If a chat message is found, it will be parsed and processed.

Output is done by simulating key presses by using the `enigo` crate. 

Note this is not tested on windows, but it should work.

## How to enable logging
- Type the following into developer's console: `con_logfile <filename>; con_timestamp 1`
- Locate the log file in the game directory

## Example implementation

This is a ping pong example, if somebody types .ping, it will respond with Pong.

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut parser = SourceCmdBuilder::new()
        .file_path(Box::new(PathBuf::from(
            "PATH_OF_LOG_FILE",
        )))
        .set_parser(Box::new(CSSLogParser::new()))
        .add_command(".ping", pong)
        .owner("USER_NAME") // This is required as it will put an input delay if you type the question.
        .build()?;

    parser.run().await?;

    Ok(())
}

async fn pong(chat_message: ChatMessage) -> Result<Option<ChatResponse>, Box<dyn std::error::Error>> {
    Ok(Some(ChatResponse::new("Pong".to_string())))
}
```

There are many other examples including a math expression evaluator, and a ChatGPT bot.

## Demo
[![DEMO](http://img.youtube.com/vi/TBfJONZ3yAE/0.jpg)](https://www.youtube.com/watch?v=TBfJONZ3yAE "SourceCmdGui Demo")

## Roadmap
This is really just a fun side project, but I would like to implement the following.
- [x] Allow for custom error handling in commands (by expecting a error that implements a custom trait)
- [x] Mutable state for commands
- [x] Allow from custom parsing

