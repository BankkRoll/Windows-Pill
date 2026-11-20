/**
 * Edit drafts over data that can also change on disk: deciding when an
 * outside change may replace what the user is editing.
 */

/**
 * Whether two JSON-shaped values hold the same content.
 *
 * @param a - First value.
 * @param b - Second value.
 */
export function sameContent<T>(a: T, b: T): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

/**
 * The draft to keep after the saved value changed from `prev` to `next`.
 * A draft with no edits of its own (still equal to `prev`) takes the new
 * value; a dirty draft is never overwritten, and a new object with the
 * same content as before changes nothing.
 *
 * @param draft - What the editor currently holds.
 * @param prev - Saved value the draft started from.
 * @param next - Saved value just received.
 * @returns `next` to adopt it, otherwise `draft` unchanged.
 * @example
 * setDraft((d) => rebaseDraft(d, previousSettings, project.settings));
 */
export function rebaseDraft<T>(draft: T, prev: T, next: T): T {
  if (sameContent(prev, next) || !sameContent(draft, prev)) return draft;
  return next;
}
