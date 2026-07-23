package main

import (
	"context"
	"log"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// demoChannel creates a streaming channel, writes some bytes into the write end, closes
// it, then reads them back from the read end — showing the channel round trip without a
// second worker. Mirrors the streaming demo in the Rust examples.
func demoChannel(client *iii.Client) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	ch, err := client.CreateChannel(ctx, nil)
	if err != nil {
		log.Printf("create channel: %v", err)
		return
	}

	// Write a binary payload and a text message, then close to signal end-of-stream.
	payload := []byte("the quick brown fox")
	if err := ch.Writer.Write(ctx, payload); err != nil {
		log.Printf("channel write: %v", err)
		return
	}
	if err := ch.Writer.SendMessage(ctx, `{"type":"progress","sent":19}`); err != nil {
		log.Printf("channel sendMessage: %v", err)
	}
	if err := ch.Writer.Close(); err != nil {
		log.Printf("channel writer close: %v", err)
	}

	// Read the stream back. Text messages (if any) go to OnMessage; binary to ReadAll.
	ch.Reader.OnMessage(func(m string) { log.Printf("channel message: %s", m) })
	got, err := ch.Reader.ReadAll(ctx)
	if err != nil {
		log.Printf("channel read: %v", err)
		return
	}
	log.Printf("channel round trip read %d bytes: %q", len(got), string(got))
}
