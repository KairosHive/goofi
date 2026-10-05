/** What the machine panel's inspector shows: the one selected state or transition, else the machine. */
export type Subject = { kind: 'machine' } | { kind: 'state'; id: string } | { kind: 'transition'; id: string };
