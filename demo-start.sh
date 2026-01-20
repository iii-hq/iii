#!/bin/bash

# iii-engine Demo Startup Script
# This script starts all components needed for the demo

set -e

echo "🚀 iii-engine Demo Setup"
echo "========================"
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Check if Redis is running
echo "📡 Checking Redis..."
if redis-cli ping > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Redis is running${NC}"
else
    echo -e "${YELLOW}⚠ Redis is not running${NC}"
    echo "Starting Redis..."
    if command -v redis-server &> /dev/null; then
        redis-server --daemonize yes
        sleep 2
        echo -e "${GREEN}✓ Redis started${NC}"
    else
        echo -e "${RED}✗ Redis not found. Please install Redis:${NC}"
        echo "  macOS: brew install redis"
        echo "  Linux: sudo apt-get install redis-server"
        exit 1
    fi
fi

echo ""
echo "🔧 Building iii-engine..."
cargo build --release

echo ""
echo "📦 Installing Node dependencies..."
cd packages/node/iii-example
pnpm install || npm install

cd ../../..

echo ""
echo -e "${GREEN}✅ Setup complete!${NC}"
echo ""
echo "========================"
echo "Starting Services:"
echo "========================"
echo ""
echo "1️⃣  Starting iii-engine..."
echo "   - WebSocket: ws://127.0.0.1:49134"
echo "   - REST API: http://127.0.0.1:3111"
echo "   - Management: http://127.0.0.1:9001"
echo "   - Streams: http://127.0.0.1:31112"
echo ""

# Start the engine in the background
cargo run &
ENGINE_PID=$!

echo "   Waiting for engine to start..."
sleep 5

echo ""
echo "2️⃣  Starting Todo Example App..."
cd packages/node/iii-example
pnpm start &
EXAMPLE_PID=$!

cd ../../..

echo ""
echo "========================"
echo -e "${GREEN}🎉 Demo Ready!${NC}"
echo "========================"
echo ""
echo "📚 Available Endpoints:"
echo "   GET    http://127.0.0.1:3111/health"
echo "   GET    http://127.0.0.1:3111/todos"
echo "   POST   http://127.0.0.1:3111/todo"
echo "   GET    http://127.0.0.1:3111/stats"
echo ""
echo "🧪 Try these commands:"
echo "   curl http://127.0.0.1:3111/health"
echo "   curl http://127.0.0.1:3111/todos"
echo '   curl -X POST http://127.0.0.1:3111/todo -H "Content-Type: application/json" -d '"'"'{"title":"Demo task","priority":"high"}'"'"
echo ""
echo "📊 Management API:"
echo "   http://127.0.0.1:9001/_console"
echo ""
echo "Press Ctrl+C to stop all services"

# Cleanup function
cleanup() {
    echo ""
    echo "🛑 Stopping services..."
    kill $ENGINE_PID 2>/dev/null || true
    kill $EXAMPLE_PID 2>/dev/null || true
    echo -e "${GREEN}✓ All services stopped${NC}"
    exit 0
}

trap cleanup INT TERM

# Wait for both processes
wait
