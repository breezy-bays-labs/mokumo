/**
 * Isolation wrapper around Runed's useSearchParams.
 * If Runed breaks or the API changes, swap the internals of this file.
 * Every vertical imports from here, never from runed/kit directly.
 */
export { useSearchParams } from "runed/kit";
