import { Components } from "react-markdown";

/** Markdown links to other sites open in a new tab; in-page anchors stay in this one. */
export const markdownLinks: Components = {
  a: ({ node, ...props }) =>
    /^https?:\/\//i.test(props.href ?? "") ? <a {...props} target="_blank" rel="noreferrer" /> : <a {...props} />,
};
