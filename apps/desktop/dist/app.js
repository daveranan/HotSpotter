const steps = [...document.querySelectorAll(".step")];
for (const step of steps) {
  step.addEventListener("click", () => {
    for (const candidate of steps) {
      candidate.classList.toggle("active", candidate === step);
    }
    document.querySelector(".status span:nth-child(2)").textContent = `Mode: ${step.textContent}`;
  });
}
