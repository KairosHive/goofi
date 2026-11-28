import { describe, it, expect } from 'vitest';
import { isTextEditingTarget } from './textEditing';

/* A text-editing surface is not always a form element: the expression editor is a
 * contenteditable `<div>`, and Ctrl+Z and Ctrl+A there belong to the text. */

/** A duck-typed target — the two fields the predicate reads, so it needs no DOM. */
const target = (tagName: string, isContentEditable = false, type?: string) =>
	({ tagName, isContentEditable, type }) as unknown as EventTarget;

describe('isTextEditingTarget', () => {
	for (const tag of ['INPUT', 'TEXTAREA', 'SELECT']) {
		it(`${tag} is a text-editing target`, () => expect(isTextEditingTarget(target(tag))).toBe(true));
	}

	it('an input that takes no letters is not: a param slider under the pointer keeps the hover keys', () => {
		for (const type of ['range', 'checkbox', 'radio', 'color']) expect(isTextEditingTarget(target('INPUT', false, type))).toBe(false);
		for (const type of ['text', 'number', 'search']) expect(isTextEditingTarget(target('INPUT', false, type))).toBe(true);
	});

	it('a contenteditable element is one too, whatever its tag', () => {
		expect(isTextEditingTarget(target('DIV', true))).toBe(true);
		expect(isTextEditingTarget(target('PRE', true))).toBe(true);
	});

	it('an ordinary element is not', () => {
		expect(isTextEditingTarget(target('DIV'))).toBe(false);
		expect(isTextEditingTarget(target('BUTTON'))).toBe(false);
		expect(isTextEditingTarget(target('BODY'))).toBe(false);
	});

	it('no target at all is not', () => {
		expect(isTextEditingTarget(null)).toBe(false);
	});
});
