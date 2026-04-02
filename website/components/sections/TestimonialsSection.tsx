import { useState, useEffect } from "react";

const testimonials = [
  {
    id: 1,
    quote: "Example testimonial",
    author: "authorname",
    role: "Founder",
    company: "bigcompany",
    avatar: "👨‍💻",
  },
  {
    id: 2,
    quote: "Example testimonial",
    author: "authorname",
    role: "Founder",
    company: "bigcompany",
    avatar: "👨‍💻",
  },
  {
    id: 3,
    quote: "Example testimonial",
    author: "authorname",
    role: "Founder",
    company: "bigcompany",
    avatar: "👨‍💻",
  },
  {
    id: 4,
    quote: "Example testimonial",
    author: "authorname",
    role: "Founder",
    company: "bigcompany",
    avatar: "👨‍💻",
  },
  {
    id: 5,
    quote: "Example testimonial",
    author: "authorname",
    role: "Founder",
    company: "bigcompany",
    avatar: "👨‍💻",
  },
];

function TestimonialCard({
  testimonial,
  isActive,
}: {
  testimonial: (typeof testimonials)[0];
  isActive: boolean;
}) {
  return (
    <div
      className={`p-8 rounded-lg border transition-all duration-300 ${
        isActive
          ? "border-green-400/30 bg-green-500/5 opacity-100"
          : "border-neutral-800 bg-neutral-900/30 opacity-50"
      }`}
    >
      {/* Stars */}
      <div className="flex gap-1 mb-4">
        {[...Array(5)].map((_, i) => (
          <span key={i} className="text-lg">
            ⭐
          </span>
        ))}
      </div>

      {/* Quote */}
      <p className="text-neutral-100 text-lg leading-relaxed mb-6">
        "{testimonial.quote}"
      </p>

      {/* Author */}
      <div className="flex items-center gap-4">
        <div className="w-12 h-12 rounded-full bg-gradient-to-br from-green-400 to-emerald-500 flex items-center justify-center text-2xl">
          {testimonial.avatar}
        </div>
        <div>
          <h4 className="font-semibold text-white">{testimonial.author}</h4>
          <p className="text-sm text-neutral-400">
            {testimonial.role} at {testimonial.company}
          </p>
        </div>
      </div>
    </div>
  );
}

function CarouselDots({
  total,
  active,
  onChange,
}: {
  total: number;
  active: number;
  onChange: (index: number) => void;
}) {
  return (
    <div className="flex justify-center gap-2">
      {[...Array(total)].map((_, index) => (
        <button
          key={index}
          onClick={() => onChange(index)}
          className={`h-2 rounded-full transition-all ${
            index === active
              ? "bg-green-400 w-8"
              : "bg-neutral-700 w-2 hover:bg-neutral-600"
          }`}
          aria-label={`Go to testimonial ${index + 1}`}
        />
      ))}
    </div>
  );
}

export function TestimonialsSection() {
  const [activeIndex, setActiveIndex] = useState(0);
  const [autoplay, setAutoplay] = useState(true);

  useEffect(() => {
    if (!autoplay) return;

    const interval = setInterval(() => {
      setActiveIndex((prev) => (prev + 1) % testimonials.length);
    }, 5000);

    return () => clearInterval(interval);
  }, [autoplay]);

  const handlePrevious = () => {
    setAutoplay(false);
    setActiveIndex(
      (prev) => (prev - 1 + testimonials.length) % testimonials.length,
    );
  };

  const handleNext = () => {
    setAutoplay(false);
    setActiveIndex((prev) => (prev + 1) % testimonials.length);
  };

  return (
    <section className="relative text-white py-24 overflow-hidden">
      <div className="relative z-10 max-w-5xl mx-auto px-6">
        {/* Header */}
        <div className="text-center mb-16 space-y-4">
          <h2 className="text-4xl md:text-5xl font-bold">
            What iii users are saying
          </h2>
          <p className="text-neutral-400 text-lg">
            Join thousands of developers using iii to build better systems
          </p>
        </div>

        {/* Carousel */}
        <div className="mb-8">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {/* Previous (show on desktop) */}
            <div className="hidden md:block">
              {testimonials[
                (activeIndex - 1 + testimonials.length) % testimonials.length
              ] && (
                <TestimonialCard
                  testimonial={
                    testimonials[
                      (activeIndex - 1 + testimonials.length) %
                        testimonials.length
                    ]
                  }
                  isActive={false}
                />
              )}
            </div>

            {/* Active */}
            <TestimonialCard
              testimonial={testimonials[activeIndex]}
              isActive={true}
            />

            {/* Next (show on desktop) */}
            <div className="hidden md:block">
              {testimonials[(activeIndex + 1) % testimonials.length] && (
                <TestimonialCard
                  testimonial={
                    testimonials[(activeIndex + 1) % testimonials.length]
                  }
                  isActive={false}
                />
              )}
            </div>
          </div>
        </div>

        {/* Controls */}
        <div className="flex items-center justify-center gap-6">
          <button
            onClick={handlePrevious}
            className="w-10 h-10 rounded-full border border-neutral-800 bg-neutral-900/50 hover:border-green-400/50 hover:bg-green-500/5 transition-colors flex items-center justify-center"
            aria-label="Previous testimonial"
          >
            <svg
              className="w-5 h-5"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M15 19l-7-7 7-7"
              />
            </svg>
          </button>

          <CarouselDots
            total={testimonials.length}
            active={activeIndex}
            onChange={(index) => {
              setAutoplay(false);
              setActiveIndex(index);
            }}
          />

          <button
            onClick={handleNext}
            className="w-10 h-10 rounded-full border border-neutral-800 bg-neutral-900/50 hover:border-green-400/50 hover:bg-green-500/5 transition-colors flex items-center justify-center"
            aria-label="Next testimonial"
          >
            <svg
              className="w-5 h-5"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M9 5l7 7-7 7"
              />
            </svg>
          </button>
        </div>

        {/* Autoplay hint */}
        <p className="text-center text-sm text-neutral-500 mt-6">
          {autoplay ? "Auto-playing testimonials..." : "Click to continue"}
        </p>
      </div>
    </section>
  );
}
