import { expect, test } from "vitest";
import { actionLabel } from "./actionLabel";

test("strips the trailing energy suffix", () => {
  expect(actionLabel("Set day/evening schedule (medium)")).toBe("Set day/evening schedule");
  expect(actionLabel("Draft methods (deep)")).toBe("Draft methods");
});

test("renders wikilinks by label or last segment", () => {
  expect(actionLabel("[[actions/integrate-tabicl-as-backbone-in-nfm]] (deep)")).toBe(
    "integrate-tabicl-as-backbone-in-nfm",
  );
  expect(actionLabel("Review [[actions/foo|the foo note]] today (light)")).toBe(
    "Review the foo note today",
  );
});

test("leaves plain text and mid-text parentheses alone", () => {
  expect(actionLabel("Call the clinic (again)")).toBe("Call the clinic (again)");
  expect(actionLabel("Buy rings")).toBe("Buy rings");
});
