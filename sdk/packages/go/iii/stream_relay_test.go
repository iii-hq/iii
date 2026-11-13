package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

// pipeRelay stands in for the engine's pipes: engine::channels::create makes one, and
// /ws/channels/{id} relays the writer's frames to the reader as the engine 0.26 does.
// A writer close with 1000 (or no code) ends the stream: the reader gets a close with
// 1000. Any other end of the writer is an abort: the reader gets 1011 with the engine's
// reason. A reader that leaves closes the writer.
type pipeRelay struct {
	mu      sync.Mutex
	pipes   map[string]*relayPipe
	created atomic.Int32
	// dials counts the stream sockets opened, by direction.
	reads, writes atomic.Int32
}

type relayFrame struct {
	typ   websocket.MessageType
	data  []byte
	end   bool
	abort string
}

type relayPipe struct {
	frames     chan relayFrame
	readerGone chan struct{}
	goneOnce   sync.Once
}

func newPipeRelay() *pipeRelay { return &pipeRelay{pipes: map[string]*relayPipe{}} }

func (r *pipeRelay) create() (writer, reader StreamChannelRef) {
	id := uuid.NewString()
	r.mu.Lock()
	r.pipes[id] = &relayPipe{frames: make(chan relayFrame, 1024), readerGone: make(chan struct{})}
	r.mu.Unlock()
	r.created.Add(1)
	return StreamChannelRef{ChannelID: id, AccessKey: "w-" + id, Direction: ChannelWrite},
		StreamChannelRef{ChannelID: id, AccessKey: "r-" + id, Direction: ChannelRead}
}

func (r *pipeRelay) ServeHTTP(w http.ResponseWriter, req *http.Request) {
	id := strings.TrimPrefix(req.URL.Path, "/ws/channels/")
	r.mu.Lock()
	p := r.pipes[id]
	r.mu.Unlock()
	dir := req.URL.Query().Get("dir")
	if p == nil || (dir != "read" && dir != "write") || req.URL.Query().Get("key") != dir[:1]+"-"+id {
		http.NotFound(w, req)
		return
	}
	conn, err := websocket.Accept(w, req, nil)
	if err != nil {
		return
	}
	conn.SetReadLimit(-1)
	ctx := req.Context()
	switch dir {
	case "write":
		r.writes.Add(1)
		go func() {
			select {
			case <-p.readerGone:
				_ = conn.Close(websocket.StatusNormalClosure, "reader left")
			case <-ctx.Done():
			}
		}()
		for {
			typ, data, err := conn.Read(ctx)
			if err != nil {
				var end relayFrame
				switch status := websocket.CloseStatus(err); status {
				case websocket.StatusNormalClosure, websocket.StatusNoStatusRcvd:
					end.end = true
				case -1:
					end.abort = "channel writer aborted: connection lost before close"
				default:
					end.abort = fmt.Sprintf("channel writer aborted: closed with code %d", int(status))
				}
				select {
				case p.frames <- end:
				case <-p.readerGone:
				}
				return
			}
			select {
			case p.frames <- relayFrame{typ: typ, data: data}:
			case <-p.readerGone:
				return
			}
		}
	case "read":
		r.reads.Add(1)
		go func() {
			for {
				if _, _, err := conn.Read(ctx); err != nil {
					p.goneOnce.Do(func() { close(p.readerGone) })
					return
				}
			}
		}()
		for {
			select {
			case f := <-p.frames:
				switch {
				case f.end:
					_ = conn.Close(websocket.StatusNormalClosure, "")
					return
				case f.abort != "":
					_ = conn.Close(websocket.StatusInternalError, f.abort)
					return
				}
				if err := conn.Write(ctx, f.typ, f.data); err != nil {
					return
				}
			case <-p.readerGone:
				return
			}
		}
	}
}

// streamEngine is the fake engine of the observe tests plus pipes, and a router that
// sends a call of a function back to this worker (for a call made with DispatchRemote),
// so one client is both the caller and the callee over real pipes.
type streamEngine struct {
	*fakeEngine
	relay *pipeRelay

	mu     sync.Mutex
	routes map[uuid.UUID]uuid.UUID
	// activations: the calls through an observer the router delivered, as activations.
	activations atomic.Int32
}

func newStreamEngine(t testing.TB) *streamEngine {
	t.Helper()
	se := &streamEngine{fakeEngine: newFakeEngine(t), relay: newPipeRelay(), routes: map[uuid.UUID]uuid.UUID{}}
	se.m.mu.Lock()
	se.m.channels = se.relay
	se.m.mu.Unlock()
	se.setOnFrame(se.route)
	return se
}

// forward sends msg to the worker, ignoring a closed connection.
func (se *streamEngine) forward(msg any) {
	se.m.mu.Lock()
	conn := se.m.active
	se.m.mu.Unlock()
	if conn != nil {
		_ = se.m.send(context.Background(), conn, msg)
	}
}

func (se *streamEngine) route(raw map[string]json.RawMessage) {
	data, err := json.Marshal(raw)
	if err != nil {
		return
	}
	d, err := UnmarshalMessage(data)
	if err != nil {
		return
	}
	switch {
	case d.InvokeFunction != nil:
		inv := d.InvokeFunction
		switch {
		case inv.FunctionID == FnCreateChannel:
			writer, reader := se.relay.create()
			result, _ := json.Marshal(map[string]StreamChannelRef{"writer": writer, "reader": reader})
			if inv.InvocationID != nil {
				se.forward(&InvocationResultMessage{InvocationID: *inv.InvocationID, FunctionID: FnCreateChannel, Result: result})
			}
		case strings.HasPrefix(inv.FunctionID, "engine::"):
		default:
			out := &InvokeFunctionMessage{FunctionID: inv.FunctionID, Data: inv.Data, Metadata: inv.Metadata}
			if inv.Activation != nil {
				se.activations.Add(1)
				out.Activation = &Activation{ObserverID: inv.Activation.ObserverID, ActivationID: "act-" + uuid.NewString()}
			}
			if inv.InvocationID != nil {
				id := uuid.New()
				se.mu.Lock()
				se.routes[id] = *inv.InvocationID
				se.mu.Unlock()
				out.InvocationID = &id
			}
			se.forward(out)
		}
	case d.InvocationResult != nil:
		res := d.InvocationResult
		se.mu.Lock()
		orig, ok := se.routes[res.InvocationID]
		delete(se.routes, res.InvocationID)
		se.mu.Unlock()
		if ok {
			se.forward(&InvocationResultMessage{InvocationID: orig, FunctionID: res.FunctionID, Result: res.Result, Error: res.Error})
		}
	}
}
