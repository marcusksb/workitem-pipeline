import { useEffect, useState } from "react";

/** Width below which the phone layout applies (decision 61); `styles.css` uses the same value. */
const PHONE = "(max-width: 700px)";

/** Whether the phone layout is on; follows rotation and window resizes. */
export function usePhone(): boolean {
  const [phone, setPhone] = useState(() => matchMedia(PHONE).matches);
  useEffect(() => {
    const query = matchMedia(PHONE);
    const onChange = () => setPhone(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return phone;
}
