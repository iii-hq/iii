package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

// maxJSONFrameBytes is the largest serialized JSON envelope sent as one text frame.
const maxJSONFrameBytes = 16 * 1024 * 1024

// maxJSONMessageBytes is the inclusive outbound and inbound JSON message limit.
const maxJSONMessageBytes = 64 * 1024 * 1024

// maxJSONFragmentBytes is the maximum payload written in each fragmented text frame.
const maxJSONFragmentBytes = 1 * 1024 * 1024

// MarshalMessage remains the public serializer; only transport adds size policy.
func prepareJSON(msg any) ([]byte, error) {
	frame, err := MarshalMessage(msg)
	if err != nil {
		return nil, err
	}
	return prepareFrame(frame)
}

func prepareFrame(frame []byte) ([]byte, error) {
	if len(frame) <= maxJSONMessageBytes {
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
	sizeErr := &InvocationError{Code: "payload_too_large", FunctionID: header.FunctionID, Message: payloadTooLargeMessage(len(frame))}
	if header.Type == "invocationresult" && header.InvocationID != nil {
		fallback, err := MarshalMessage(&InvocationResultMessage{InvocationID: *header.InvocationID, FunctionID: header.FunctionID,
			Error: &ErrorBody{Code: sizeErr.Code, Message: sizeErr.Message}})
		if err != nil {
			return frame, err
		}
		if len(fallback) <= maxJSONMessageBytes {
			return fallback, nil
		}
	}
	return frame, sizeErr
}

// payloadTooLargeMessage is the payload_too_large message for a JSON message of size
// bytes.
func payloadTooLargeMessage(size int) string {
	return fmt.Sprintf("Serialized JSON envelope is %d bytes; limit is %d bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size", size, maxJSONMessageBytes)
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
	if len(checked) <= maxJSONFrameBytes {
		return conn.Write(ctx, websocket.MessageText, checked)
	}

	writer, err := conn.Writer(ctx, websocket.MessageText)
	if err != nil {
		return err
	}
	for start := 0; start < len(checked); start += maxJSONFragmentBytes {
		end := start + maxJSONFragmentBytes
		if end > len(checked) {
			end = len(checked)
		}
		n, writeErr := writer.Write(checked[start:end])
		if writeErr != nil {
			return writeErr
		}
		if n != end-start {
			return io.ErrShortWrite
		}
	}
	return writer.Close()
}
