// Package sample is a fixture for the godoc-json tests.
//
// Second paragraph.
package sample

import "encoding/json"

// DefaultURL is the default address.
const DefaultURL = "ws://127.0.0.1:1"

// Limits of the sample.
const (
	// MaxA bounds A.
	MaxA = 8
	// OldB bounds B.
	//
	// Deprecated: use [MaxA] instead. See https://example.com/migrate.
	OldB = 1 << 3
)

// Level is a level.
type Level int

// Levels.
const (
	LevelLow Level = iota
	LevelHigh
)

// Option configures a [Client].
type Option func(*Client)

// WithName sets the name.
func WithName(name string) Option { return func(c *Client) { c.name = name } }

// Alias is the same type as Message.
type Alias = Message

// Message is a wire message.
type Message struct {
	// ID is required.
	ID string `json:"id"`
	// Note is optional on the wire.
	Note   *string         `json:"note,omitempty"`
	Data   json.RawMessage `json:"data,omitempty"` // trailing comment doc
	A, B   int
	hidden bool
}

// Client is a client.
type Client struct {
	name string
}

// New creates a client.
func New(url string, opts ...Option) *Client { return &Client{} }

// Send sends data.
//
//	c.Send(nil)
func (c *Client) Send(data []byte) error { return nil }

// Legacy sends the old way.
//
// Deprecated: use [Client.Send].
func (c *Client) Legacy(id,
	name string) error {
	return nil
}

// Doer does things.
type Doer interface {
	// Do does one thing.
	Do(n int) (ok bool, err error)
}
