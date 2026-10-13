/** What the machine panel's inspector shows: the selected states, or the selected transitions. */
export type Subject = { kind: 'state' | 'transition'; ids: string[] };
