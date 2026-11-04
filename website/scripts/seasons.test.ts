import assert from "node:assert/strict"
import test from "node:test"
import { diwaliWindow, seasonFromQuery, seasonOn, thanksgivingDay } from "../lib/seasons"

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

test("Diwali covers Dhanteras to Bhai Dooj, inclusive, and wins over Thanksgiving", () => {
  assert.equal(seasonOn(day(2026, 11, 5)), "thanksgiving")
  assert.equal(seasonOn(day(2026, 11, 6, 0)), "diwali")
  assert.equal(seasonOn(day(2026, 11, 8)), "diwali")
  assert.equal(seasonOn(day(2026, 11, 11, 23)), "diwali")
  assert.equal(seasonOn(day(2026, 11, 12, 0)), "thanksgiving")
})

test("Diwali follows its table in later years", () => {
  assert.equal(seasonOn(day(2027, 10, 26)), null)
  assert.equal(seasonOn(day(2027, 10, 27)), "diwali")
  assert.equal(seasonOn(day(2027, 10, 31, 23)), "diwali")
  assert.equal(seasonOn(day(2027, 11, 1)), null)
  assert.equal(seasonOn(day(2028, 10, 17)), "diwali")
  assert.equal(seasonOn(day(2029, 11, 5)), "diwali")
  assert.equal(seasonOn(day(2030, 10, 26)), "diwali")
  assert.equal(diwaliWindow(2031), null)
})

test("seasonFromQuery reads the preview override", () => {
  assert.equal(seasonFromQuery("?season=diwali"), "diwali")
  assert.equal(seasonFromQuery("?season=thanksgiving"), "thanksgiving")
  assert.equal(seasonFromQuery("?season=off"), null)
  assert.equal(seasonFromQuery("?season=halloween"), undefined)
  assert.equal(seasonFromQuery("?utm_source=x"), undefined)
  assert.equal(seasonFromQuery(""), undefined)
})
