package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"log"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

const maxJSONFrameBytes = 16 * 1024 * 1024
const maxJSONMessageBytes = 64 * 1024 * 1024

// MarshalMessage remains the public serializer; only transport adds size policy.
func prepareJSON(msg any) ([]byte, error) {
	frame, err := MarshalMessage(msg)
	if err != nil {
		return nil, err
	}
	return prepareFrame(frame)
}

func prepareFrame(frame []byte) ([]byte, error) {
	if len(frame) <= maxJSONFrameBytes {
		return frame, nil
	}
	var header struct {
		Type         string     `json:"type"`
		InvocationID *uuid.UUID `json:"invocation_id"`
		FunctionID   string     `json:"function_id"`
	}
	if err := json.Unmarshal(frame, &header); err != nil {
		return frame, err
	}
	sizeErr := &InvocationError{Code: "payload_too_large", FunctionID: header.FunctionID,
		Message: fmt.Sprintf("Serialized JSON envelope is %d bytes; limit is %d bytes. Use channels for large data.", len(frame), maxJSONFrameBytes)}
	if header.Type == "invocationresult" && header.InvocationID != nil {
		fallback, err := MarshalMessage(&InvocationResultMessage{InvocationID: *header.InvocationID, FunctionID: header.FunctionID,
			Error: &ErrorBody{Code: sizeErr.Code, Message: sizeErr.Message}})
		if err != nil {
			return frame, err
		}
		if len(fallback) <= maxJSONFrameBytes {
			return fallback, nil
		}
	}
	return frame, sizeErr
}

func (c *Client) rejectFrame(frame []byte, err error) {
	// Only parse the correlation header; never log rejected JSON.
	var header struct {
		Type         string     `json:"type"`
		InvocationID *uuid.UUID `json:"invocation_id"`
	}
	if json.Unmarshal(frame, &header) == nil && header.InvocationID != nil && header.Type == "invokefunction" {
		c.mu.Lock()
		pending := c.pending[*header.InvocationID]
		delete(c.pending, *header.InvocationID)
		c.mu.Unlock()
		if pending != nil {
			pending <- invocationOutcome{err: err}
		}
	}
	if sizeErr, ok := err.(*InvocationError); ok {
		log.Printf("iii: JSON envelope rejected: %s: %s", sizeErr.Code, sizeErr.Message)
	} else {
		log.Print("iii: JSON envelope serialization rejected")
	}
}

func (c *Client) writeJSON(ctx context.Context, conn *websocket.Conn, frame []byte) error {
	checked, err := prepareFrame(frame)
	if err != nil {
		return err
	}
	return conn.Write(ctx, websocket.MessageText, checked)
}
