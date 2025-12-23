#!/bin/bash

# Load environment variables from .env file
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
    echo "Loaded .env file"
else
    echo "Error: .env file not found!"
    echo "Copy .env.example to .env and fill in your values:"
    echo "  cp .env.example .env"
    exit 1
fi

# Check required variables
if [ -z "$BOT_TOKEN" ] || [ "$BOT_TOKEN" = "your_bot_token_here" ]; then
    echo "Error: BOT_TOKEN not set in .env"
    exit 1
fi

if [ -z "$DD_TOKEN" ] || [ "$DD_TOKEN" = "your_dd_token_here" ]; then
    echo "Error: DD_TOKEN not set in .env"
    exit 1
fi

if [ -z "$DISCORD_PUBLIC_KEY" ] || [ "$DISCORD_PUBLIC_KEY" = "your_public_key_here" ]; then
    echo "Error: DISCORD_PUBLIC_KEY not set in .env"
    exit 1
fi

echo "Starting DD Discord Bot..."
echo "Server will listen on http://127.0.0.1:8080"
echo ""
echo "Make sure ngrok is running: ngrok http 8080"
echo "And set your Interactions Endpoint URL in Discord Developer Portal"
echo ""

cargo run
