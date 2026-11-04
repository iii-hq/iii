import assert from "node:assert/strict"
import test from "node:test"
import { seasonFromQuery, seasonOn, thanksgivingDay } from "../lib/seasons"

const day = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h)

test("thanksgivingDay is the fourth Thursday of November", () => {
  assert.equal(thanksgivingDay(2025).getDate(), 27)
  assert.equal(thanksgivingDay(2026).getDate(), 26)
  assert.equal(thanksgivingDay(2027).getDate(), 25)
  assert.equal(thanksgivingDay(2028).getDate(), 23)
  assert.equal(thanksgivingDay(2029).getDate(), 22)
  for (const year of [2025, 2026, 2027, 2028, 2029]) {
    assert.equal(thanksgivingDay(year).getDay(), 4)
  }
})

test("2026 starts early on Nov 5 and still ends after Thanksgiving week, on its own", () => {
  assert.equal(seasonOn(day(2026, 11, 4, 23)), null)
  assert.equal(seasonOn(day(2026, 11, 5, 0)), "thanksgiving")
  assert.equal(seasonOn(day(2026, 11, 22)), "thanksgiving")
  assert.equal(seasonOn(day(2026, 11, 26)), "thanksgiving")
  assert.equal(seasonOn(day(2026, 11, 29, 23)), "thanksgiving")
  assert.equal(seasonOn(day(2026, 11, 30, 0)), null)
})

test("seasonOn is off the rest of the year", () => {
  assert.equal(seasonOn(day(2026, 10, 31)), null)
  assert.equal(seasonOn(day(2026, 12, 25)), null)
  assert.equal(seasonOn(day(2027, 7, 4)), null)
  assert.equal(seasonOn(day(2027, 11, 5)), null)
})

test("other years cover Monday before to Sunday after Thanksgiving, inclusive", () => {
  assert.equal(seasonOn(day(2027, 11, 22)), "thanksgiving")
  assert.equal(seasonOn(day(2027, 11, 21)), null)
  assert.equal(seasonOn(day(2027, 11, 28, 23)), "thanksgiving")
  assert.equal(seasonOn(day(2027, 11, 29)), null)
})

test("seasonFromQuery reads the preview override", () => {
  assert.equal(seasonFromQuery("?season=thanksgiving"), "thanksgiving")
  assert.equal(seasonFromQuery("?season=off"), null)
  assert.equal(seasonFromQuery("?season=halloween"), undefined)
  assert.equal(seasonFromQuery("?utm_source=x"), undefined)
  assert.equal(seasonFromQuery(""), undefined)
})
