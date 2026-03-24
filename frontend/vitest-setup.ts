import '@testing-library/jest-dom/vitest';

// jsdom doesn't fully implement HTMLDialogElement — polyfill for ConfirmDialog tests
HTMLDialogElement.prototype.showModal ??= function () {
	(this as HTMLDialogElement).open = true;
};
HTMLDialogElement.prototype.close ??= function () {
	(this as HTMLDialogElement).open = false;
};
