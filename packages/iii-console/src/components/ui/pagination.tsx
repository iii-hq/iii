'use client';

import { useState } from 'react';
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from 'lucide-react';
import { Button } from './card';

interface PaginationProps {
  currentPage: number;
  totalPages: number;
  totalItems: number;
  pageSize: number;
  onPageChange: (page: number) => void;
  onPageSizeChange?: (size: number) => void;
  pageSizeOptions?: number[];
  showPageSize?: boolean;
  className?: string;
}

export function Pagination({
  currentPage,
  totalPages,
  totalItems,
  pageSize,
  onPageChange,
  onPageSizeChange,
  pageSizeOptions = [25, 50, 100, 250],
  showPageSize = true,
  className = '',
}: PaginationProps) {
  const startItem = (currentPage - 1) * pageSize + 1;
  const endItem = Math.min(currentPage * pageSize, totalItems);

  return (
    <div className={`flex items-center justify-between gap-4 ${className}`}>
      <div className="flex items-center gap-2 text-[10px] md:text-xs text-muted">
        <span>
          {startItem}-{endItem} of {totalItems.toLocaleString()}
        </span>
      </div>

      <div className="flex items-center gap-1 md:gap-2">
        {showPageSize && onPageSizeChange && (
          <div className="flex items-center gap-1.5 mr-2 md:mr-4">
            <span className="text-[10px] md:text-xs text-muted hidden sm:inline">Show:</span>
            <select
              value={pageSize}
              onChange={(e) => onPageSizeChange(Number(e.target.value))}
              className="h-6 md:h-7 px-1.5 md:px-2 text-[10px] md:text-xs bg-dark-gray border border-border rounded focus:outline-none focus:ring-1 focus:ring-primary"
            >
              {pageSizeOptions.map((size) => (
                <option key={size} value={size}>
                  {size}
                </option>
              ))}
            </select>
          </div>
        )}

        <Button
          variant="ghost"
          size="sm"
          onClick={() => onPageChange(1)}
          disabled={currentPage === 1}
          className="h-6 md:h-7 w-6 md:w-7 p-0"
          title="First page"
        >
          <ChevronsLeft className="w-3 h-3 md:w-3.5 md:h-3.5" />
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={() => onPageChange(currentPage - 1)}
          disabled={currentPage === 1}
          className="h-6 md:h-7 w-6 md:w-7 p-0"
          title="Previous page"
        >
          <ChevronLeft className="w-3 h-3 md:w-3.5 md:h-3.5" />
        </Button>

        <div className="flex items-center gap-1 px-1 md:px-2">
          <span className="text-[10px] md:text-xs font-medium tabular-nums">
            {currentPage}
          </span>
          <span className="text-[10px] md:text-xs text-muted">/</span>
          <span className="text-[10px] md:text-xs text-muted tabular-nums">
            {totalPages}
          </span>
        </div>

        <Button
          variant="ghost"
          size="sm"
          onClick={() => onPageChange(currentPage + 1)}
          disabled={currentPage === totalPages}
          className="h-6 md:h-7 w-6 md:w-7 p-0"
          title="Next page"
        >
          <ChevronRight className="w-3 h-3 md:w-3.5 md:h-3.5" />
        </Button>

        <Button
          variant="ghost"
          size="sm"
          onClick={() => onPageChange(totalPages)}
          disabled={currentPage === totalPages}
          className="h-6 md:h-7 w-6 md:w-7 p-0"
          title="Last page"
        >
          <ChevronsRight className="w-3 h-3 md:w-3.5 md:h-3.5" />
        </Button>
      </div>
    </div>
  );
}

// Hook for pagination state management
export function usePagination<T>(items: T[], defaultPageSize = 50) {
  const [currentPage, setCurrentPage] = useState(1);
  const [pageSize, setPageSize] = useState(defaultPageSize);

  const totalPages = Math.max(1, Math.ceil(items.length / pageSize));
  
  // Reset to page 1 if current page exceeds total pages
  if (currentPage > totalPages) {
    setCurrentPage(1);
  }

  const paginatedItems = items.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize
  );

  const handlePageChange = (page: number) => {
    setCurrentPage(Math.max(1, Math.min(page, totalPages)));
  };

  const handlePageSizeChange = (size: number) => {
    setPageSize(size);
    setCurrentPage(1); // Reset to first page when changing page size
  };

  return {
    currentPage,
    pageSize,
    totalPages,
    totalItems: items.length,
    paginatedItems,
    onPageChange: handlePageChange,
    onPageSizeChange: handlePageSizeChange,
  };
}
