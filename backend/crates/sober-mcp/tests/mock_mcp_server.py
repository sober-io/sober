#!/usr/bin/env python3
"""Mock MCP server for integration testing.

Speaks JSON-RPC 2.0 over stdio, implementing the MCP 2025-03-26 protocol.
Supports: initialize, tools/list, tools/call, resources/list, resources/read.
"""

import json
import sys


def send_response(response):
    """Write a JSON-RPC response to stdout."""
    line = json.dumps(response) + "\n"
    sys.stdout.write(line)
    sys.stdout.flush()


def handle_initialize(req_id, params):
    """Handle the initialize request."""
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": {},
                "resources": {},
            },
            "serverInfo": {
                "name": "mock-mcp-server",
                "version": "1.0.0",
            },
        },
    }


def handle_tools_list(req_id, params):
    """Handle tools/list."""
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {
            "tools": [
                {
                    "name": "echo",
                    "description": "Echoes back the input text",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string", "description": "Text to echo"},
                        },
                        "required": ["text"],
                    },
                },
                {
                    "name": "add",
                    "description": "Adds two numbers",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "a": {"type": "number"},
                            "b": {"type": "number"},
                        },
                        "required": ["a", "b"],
                    },
                },
            ]
        },
    }


def handle_tools_call(req_id, params):
    """Handle tools/call."""
    name = params.get("name", "")
    arguments = params.get("arguments", {})

    if name == "echo":
        text = arguments.get("text", "")
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [{"type": "text", "text": f"Echo: {text}"}],
                "isError": False,
            },
        }
    elif name == "add":
        a = arguments.get("a", 0)
        b = arguments.get("b", 0)
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [{"type": "text", "text": str(a + b)}],
                "isError": False,
            },
        }
    else:
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {
                "code": -32601,
                "message": f"Unknown tool: {name}",
            },
        }


def handle_resources_list(req_id, params):
    """Handle resources/list."""
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {
            "resources": [
                {
                    "uri": "test://greeting",
                    "name": "greeting",
                    "description": "A greeting message",
                    "mimeType": "text/plain",
                },
                {
                    "uri": "test://data",
                    "name": "data",
                    "description": "Some JSON data",
                    "mimeType": "application/json",
                },
            ]
        },
    }


def handle_resources_read(req_id, params):
    """Handle resources/read."""
    uri = params.get("uri", "")

    if uri == "test://greeting":
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "contents": [
                    {
                        "uri": "test://greeting",
                        "mimeType": "text/plain",
                        "text": "Hello from mock MCP server!",
                    }
                ]
            },
        }
    elif uri == "test://data":
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "contents": [
                    {
                        "uri": "test://data",
                        "mimeType": "application/json",
                        "text": '{"key": "value", "count": 42}',
                    }
                ]
            },
        }
    else:
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {
                "code": -32602,
                "message": f"Unknown resource: {uri}",
            },
        }


HANDLERS = {
    "initialize": handle_initialize,
    "tools/list": handle_tools_list,
    "tools/call": handle_tools_call,
    "resources/list": handle_resources_list,
    "resources/read": handle_resources_read,
}


def main():
    """Main loop: read JSON-RPC requests from stdin, write responses to stdout."""
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            send_response(
                {
                    "jsonrpc": "2.0",
                    "id": None,
                    "error": {"code": -32700, "message": "Parse error"},
                }
            )
            continue

        method = request.get("method", "")
        req_id = request.get("id")
        params = request.get("params", {})

        # Notifications (no id) don't get responses.
        if req_id is None:
            continue

        handler = HANDLERS.get(method)
        if handler:
            response = handler(req_id, params)
        else:
            response = {
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32601,
                    "message": f"Method not found: {method}",
                },
            }

        send_response(response)


if __name__ == "__main__":
    main()
