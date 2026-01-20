#!/bin/bash

# Quick Demo Test Script
# Run this after demo-start.sh to test all features

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🧪 iii-engine Demo Tests${NC}"
echo "========================"
echo ""

# Wait for services to be ready
echo "⏳ Waiting for services to be ready..."
sleep 2

# Test 1: Health Check
echo -e "${YELLOW}Test 1: Health Check${NC}"
response=$(curl -s http://127.0.0.1:3111/health)
if echo "$response" | grep -q "healthy"; then
    echo -e "${GREEN}✓ Health check passed${NC}"
    echo "   Response: $response"
else
    echo "✗ Health check failed"
    exit 1
fi
echo ""

# Test 2: Create Todo
echo -e "${YELLOW}Test 2: Create Todo${NC}"
response=$(curl -s -X POST http://127.0.0.1:3111/todo \
  -H "Content-Type: application/json" \
  -d '{"title":"Test Demo Task","description":"Testing iii-engine","priority":"high"}')

if echo "$response" | grep -q "Test Demo Task"; then
    echo -e "${GREEN}✓ Todo created successfully${NC}"
    todo_id=$(echo "$response" | grep -o '"id":"[^"]*"' | cut -d'"' -f4)
    echo "   Created todo: $todo_id"
else
    echo "✗ Failed to create todo"
    exit 1
fi
echo ""

# Test 3: List Todos
echo -e "${YELLOW}Test 3: List Todos${NC}"
response=$(curl -s http://127.0.0.1:3111/todos)
if echo "$response" | grep -q "Test Demo Task"; then
    count=$(echo "$response" | grep -o '"count":[0-9]*' | cut -d':' -f2)
    echo -e "${GREEN}✓ Listed todos successfully${NC}"
    echo "   Total todos: $count"
else
    echo "✗ Failed to list todos"
    exit 1
fi
echo ""

# Test 4: Get Specific Todo
echo -e "${YELLOW}Test 4: Get Specific Todo${NC}"
if [ ! -z "$todo_id" ]; then
    response=$(curl -s http://127.0.0.1:3111/todo/$todo_id)
    if echo "$response" | grep -q "$todo_id"; then
        echo -e "${GREEN}✓ Retrieved todo successfully${NC}"
    else
        echo "✗ Failed to get todo"
        exit 1
    fi
else
    echo "⚠ Skipping (no todo_id)"
fi
echo ""

# Test 5: Update Todo
echo -e "${YELLOW}Test 5: Update Todo${NC}"
if [ ! -z "$todo_id" ]; then
    response=$(curl -s -X PUT http://127.0.0.1:3111/todo/$todo_id \
      -H "Content-Type: application/json" \
      -d '{"description":"Updated during demo test"}')
    
    if echo "$response" | grep -q "Updated during demo test"; then
        echo -e "${GREEN}✓ Updated todo successfully${NC}"
    else
        echo "✗ Failed to update todo"
        exit 1
    fi
else
    echo "⚠ Skipping (no todo_id)"
fi
echo ""

# Test 6: Complete Todo
echo -e "${YELLOW}Test 6: Complete Todo${NC}"
if [ ! -z "$todo_id" ]; then
    response=$(curl -s -X POST http://127.0.0.1:3111/todo/$todo_id/complete)
    
    if echo "$response" | grep -q "completed"; then
        echo -e "${GREEN}✓ Completed todo successfully${NC}"
    else
        echo "✗ Failed to complete todo"
        exit 1
    fi
else
    echo "⚠ Skipping (no todo_id)"
fi
echo ""

# Test 7: Get Stats
echo -e "${YELLOW}Test 7: Get Statistics${NC}"
response=$(curl -s http://127.0.0.1:3111/stats)
if echo "$response" | grep -q "stats"; then
    echo -e "${GREEN}✓ Retrieved stats successfully${NC}"
    echo "   Stats: $response"
else
    echo "✗ Failed to get stats"
    exit 1
fi
echo ""

# Test 8: Delete Todo
echo -e "${YELLOW}Test 8: Delete Todo${NC}"
if [ ! -z "$todo_id" ]; then
    response=$(curl -s -X DELETE http://127.0.0.1:3111/todo/$todo_id)
    
    if echo "$response" | grep -q "success"; then
        echo -e "${GREEN}✓ Deleted todo successfully${NC}"
    else
        echo "✗ Failed to delete todo"
        exit 1
    fi
else
    echo "⚠ Skipping (no todo_id)"
fi
echo ""

# Summary
echo "========================"
echo -e "${GREEN}✅ All tests passed!${NC}"
echo "========================"
echo ""
echo "🎉 Demo is ready for presentation!"
echo ""
echo "💡 Tip: Run 'curl http://127.0.0.1:3111/stats' to see current state"
