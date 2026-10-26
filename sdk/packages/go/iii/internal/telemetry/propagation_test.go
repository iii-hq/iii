package telemetry

import (
	"context"
	"testing"

	"go.opentelemetry.io/otel/attribute"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"
)

const fixtureTraceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"

func TestInjectExtractRoundTrip(t *testing.T) {
	if tp, bag := Inject(context.Background()); tp != "" || bag != "" {
		t.Errorf("empty ctx injected %q %q", tp, bag)
	}
	ctx := Extract(context.Background(), fixtureTraceparent, "tenant=acme,user=u%201")
	sc := trace.SpanContextFromContext(ctx)
	if !sc.IsRemote() || sc.TraceID() != fixtureTraceID || sc.SpanID() != fixtureSpanID || !sc.IsSampled() {
		t.Errorf("extracted span context %+v", sc)
	}
	if v, ok := GetBaggageEntry(ctx, "user"); !ok || v != "u 1" {
		t.Errorf("baggage user = %q %v", v, ok)
	}
	tp, bag := Inject(ctx)
	if tp != fixtureTraceparent {
		t.Errorf("traceparent %q", tp)
	}
	if got := Extract(context.Background(), "", bag); GetAllBaggage(got)["tenant"] != "acme" || GetAllBaggage(got)["user"] != "u 1" {
		t.Errorf("baggage %q did not round-trip", bag)
	}
	if CurrentTraceID(ctx) != fixtureTraceID.String() || CurrentSpanID(ctx) != fixtureSpanID.String() {
		t.Errorf("current ids %q %q", CurrentTraceID(ctx), CurrentSpanID(ctx))
	}
	if CurrentTraceID(context.Background()) != "" || CurrentSpanID(context.Background()) != "" {
		t.Error("ids without a span")
	}
	// Invalid headers leave ctx unchanged.
	if bad := Extract(context.Background(), "00-zz-yy-01", "="); trace.SpanContextFromContext(bad).IsValid() || len(GetAllBaggage(bad)) != 0 {
		t.Error("invalid headers changed ctx")
	}
}

func TestBaggageHelpers(t *testing.T) {
	ctx, err := SetBaggageEntry(context.Background(), "a", "1")
	if err != nil {
		t.Fatal(err)
	}
	ctx, _ = SetBaggageEntry(ctx, "b", "two words")
	if all := GetAllBaggage(ctx); len(all) != 2 || all["a"] != "1" || all["b"] != "two words" {
		t.Errorf("all baggage %v", all)
	}
	ctx = RemoveBaggageEntry(ctx, "a")
	if _, ok := GetBaggageEntry(ctx, "a"); ok {
		t.Error("a still present")
	}
	if _, err := SetBaggageEntry(ctx, "", "x"); err == nil {
		t.Error("invalid key accepted")
	}
	if got := RemoveBaggageEntry(context.Background(), "x"); got != context.Background() {
		t.Error("removing from empty baggage changed ctx")
	}
}

func TestBaggageSpanProcessorCopiesBaggage(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tp := sdktrace.NewTracerProvider(sdktrace.WithSpanProcessor(BaggageSpanProcessor{}), sdktrace.WithSpanProcessor(rec))
	ctx, _ := SetBaggageEntry(context.Background(), "iii.tag.env", "prod")
	_, span := tp.Tracer("t").Start(ctx, "s")
	span.End()
	attrs := rec.Ended()[0].Attributes()
	if len(attrs) != 1 || attrs[0] != attribute.String("iii.tag.env", "prod") {
		t.Errorf("attributes %v", attrs)
	}
}

func TestSpanOps(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tp := sdktrace.NewTracerProvider(sdktrace.WithSpanProcessor(rec))
	ctx, span := tp.Tracer("t").Start(context.Background(), "s")
	if !CurrentSpanIsRecording(ctx) || CurrentSpanIsRecording(context.Background()) {
		t.Error("CurrentSpanIsRecording")
	}
	SetCurrentSpanAttribute(ctx, "n", 3)
	SetCurrentSpanAttribute(ctx, "tags", []string{"a"})
	RecordSpanEvent(ctx, "evt", map[string]any{"ok": true})
	SetCurrentSpanError(ctx, "bad")
	SetCurrentSpanAttribute(context.Background(), "ignored", 1)
	span.End()
	ended := rec.Ended()[0]
	if got := ended.Attributes(); len(got) != 2 || got[0] != attribute.Int("n", 3) {
		t.Errorf("attributes %v", got)
	}
	if evs := ended.Events(); len(evs) != 1 || evs[0].Name != "evt" || evs[0].Attributes[0] != attribute.Bool("ok", true) {
		t.Errorf("events %v", evs)
	}
	if ended.Status().Description != "bad" {
		t.Errorf("status %+v", ended.Status())
	}
}
